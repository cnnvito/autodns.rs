use crate::desktop::DnsLookupRecord;
use anyhow::{anyhow, Context, Result};
use hickory_proto::op::{
    Message as DnsMessage, MessageType as DnsMessageType, OpCode as DnsOpCode, Query as DnsQuery,
    ResponseCode as DnsResponseCode,
};
use hickory_proto::rr::rdata::{A as DnsA, AAAA as DnsAaaa};
use hickory_proto::rr::{
    DNSClass, Name as DnsName, RData as DnsRData, Record as DnsRecord, RecordType as DnsRecordType,
};
use std::sync::Arc;

pub(crate) const TYPE_A: u16 = 1;
pub(crate) const TYPE_NS: u16 = 2;
pub(crate) const TYPE_CNAME: u16 = 5;
pub(crate) const TYPE_SOA: u16 = 6;
pub(crate) const TYPE_MX: u16 = 15;
pub(crate) const TYPE_TXT: u16 = 16;
pub(crate) const TYPE_AAAA: u16 = 28;
pub(crate) const TYPE_HTTPS: u16 = 65;
#[cfg(test)]
pub(crate) const CLASS_IN: u16 = 1;
pub(crate) const RCODE_SUCCESS: u8 = 0;
pub(crate) const RCODE_NAME_ERROR: u8 = 3;
pub(crate) const RCODE_SERVER_FAILURE: u8 = 2;
pub(crate) const RCODE_REFUSED: u8 = 5;
pub(crate) const DNS_WIRE_LIMIT: usize = 65535;
#[derive(Clone)]
pub(crate) struct Question {
    pub(crate) id: u16,
    pub(crate) normalized_qname: String,
    pub(crate) qtype: u16,
    pub(crate) qclass: u16,
    pub(crate) query: DnsQuery,
    pub(crate) op_code: DnsOpCode,
    pub(crate) recursion_desired: bool,
    pub(crate) checking_disabled: bool,
}

pub(crate) fn parse_question(req: &[u8]) -> Result<Question> {
    if req.len() < 12 {
        return Err(anyhow!("dns query is too short"));
    }
    let qdcount = u16::from_be_bytes([req[4], req[5]]);
    if qdcount != 1 {
        return Err(anyhow!("resolver only supports single-question requests"));
    }
    let id = u16::from_be_bytes([req[0], req[1]]);
    let flags = u16::from_be_bytes([req[2], req[3]]);
    let (qname, offset) = parse_question_name(req, 12)?;
    if offset + 4 > req.len() {
        return Err(anyhow!("dns question is truncated"));
    }
    let qtype = u16::from_be_bytes([req[offset], req[offset + 1]]);
    let qclass = u16::from_be_bytes([req[offset + 2], req[offset + 3]]);
    let name = if qname.is_empty() {
        DnsName::root()
    } else {
        DnsName::from_ascii(&qname).context("decode dns question name")?
    };
    let mut query = DnsQuery::query(name, DnsRecordType::from(qtype));
    query.set_query_class(DNSClass::from(qclass));
    Ok(Question {
        id,
        normalized_qname: normalize_domain_lossy(&qname),
        qtype,
        qclass,
        query,
        op_code: DnsOpCode::from_u8(((flags >> 11) & 0x0f) as u8),
        recursion_desired: flags & 0x0100 != 0,
        checking_disabled: flags & 0x0010 != 0,
    })
}

pub(crate) fn parse_question_name(req: &[u8], offset: usize) -> Result<(String, usize)> {
    let mut name = String::new();
    let mut pos = offset;
    let mut next_offset = None;
    let mut jumps = 0usize;

    loop {
        let len = *req
            .get(pos)
            .ok_or_else(|| anyhow!("dns name is truncated"))?;
        if len & 0b1100_0000 == 0b1100_0000 {
            let next = *req
                .get(pos + 1)
                .ok_or_else(|| anyhow!("dns compression pointer is truncated"))?;
            let pointer = (((len & 0b0011_1111) as usize) << 8) | next as usize;
            if pointer >= req.len() {
                return Err(anyhow!("dns compression pointer is out of bounds"));
            }
            next_offset.get_or_insert(pos + 2);
            pos = pointer;
            jumps += 1;
            if jumps > req.len() {
                return Err(anyhow!("dns compression pointer loop detected"));
            }
            continue;
        }
        if len & 0b1100_0000 != 0 {
            return Err(anyhow!("unsupported dns name label type"));
        }
        pos += 1;
        if len == 0 {
            return Ok((name, next_offset.unwrap_or(pos)));
        }
        let end = pos + len as usize;
        if end > req.len() {
            return Err(anyhow!("dns name label is truncated"));
        }
        let label =
            std::str::from_utf8(&req[pos..end]).context("dns name label is not valid utf-8")?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(label);
        pos = end;
    }
}

pub(crate) fn hosts_response(question: &Question, ipv4: &[[u8; 4]], ipv6: &[[u8; 16]]) -> Vec<u8> {
    let name = DnsName::from_ascii(&question.normalized_qname).unwrap_or_else(|_| DnsName::root());
    let mut resp = response_for_question(question, DnsResponseCode::NoError);
    for ip in ipv4 {
        resp.add_answer(DnsRecord::from_rdata(
            name.clone(),
            60,
            DnsRData::A(DnsA::new(ip[0], ip[1], ip[2], ip[3])),
        ));
    }
    for ip in ipv6 {
        resp.add_answer(DnsRecord::from_rdata(
            name.clone(),
            60,
            DnsRData::AAAA(DnsAaaa::from(std::net::Ipv6Addr::from(*ip))),
        ));
    }
    resp.to_vec()
        .unwrap_or_else(|_| servfail_response_for_question(question))
}

#[cfg(test)]
pub(crate) fn write_rr_header(resp: &mut Vec<u8>, qtype: u16, qclass: u16, ttl: u32, rdlen: u16) {
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&qclass.to_be_bytes());
    resp.extend_from_slice(&ttl.to_be_bytes());
    resp.extend_from_slice(&rdlen.to_be_bytes());
}

pub(crate) fn servfail_response(req: &[u8]) -> Vec<u8> {
    response_for_query(req, DnsResponseCode::ServFail)
        .to_vec()
        .unwrap_or_else(|_| req.to_vec())
}

pub(crate) fn servfail_response_for_question(question: &Question) -> Vec<u8> {
    response_for_question(question, DnsResponseCode::ServFail)
        .to_vec()
        .unwrap_or_default()
}

pub(crate) fn empty_success_response(question: &Question) -> Vec<u8> {
    response_for_question(question, DnsResponseCode::NoError)
        .to_vec()
        .unwrap_or_else(|_| servfail_response_for_question(question))
}

pub(crate) fn build_query(domain: &str, qtype: u16) -> Vec<u8> {
    let domain = normalize_domain_lossy(domain);
    let name = if domain.is_empty() {
        DnsName::root()
    } else {
        DnsName::from_ascii(&domain).unwrap_or_else(|_| DnsName::root())
    };
    let mut msg = DnsMessage::new(0x1234, DnsMessageType::Query, DnsOpCode::Query);
    msg.metadata.recursion_desired = true;
    msg.add_query(DnsQuery::query(name, DnsRecordType::from(qtype)));
    msg.to_vec().unwrap_or_default()
}

pub(crate) fn lookup_qtype(record_type: &str) -> Result<u16> {
    match record_type.trim().to_ascii_uppercase().as_str() {
        "A" => Ok(TYPE_A),
        "AAAA" => Ok(TYPE_AAAA),
        "CNAME" => Ok(TYPE_CNAME),
        "MX" => Ok(TYPE_MX),
        "TXT" => Ok(TYPE_TXT),
        "NS" => Ok(TYPE_NS),
        "SOA" => Ok(TYPE_SOA),
        "HTTPS" => Ok(TYPE_HTTPS),
        value => Err(anyhow!("unsupported DNS record type {value:?}")),
    }
}

pub(crate) fn qtype_label(qtype: u16) -> &'static str {
    match qtype {
        TYPE_A => "A",
        TYPE_AAAA => "AAAA",
        TYPE_CNAME => "CNAME",
        TYPE_MX => "MX",
        TYPE_TXT => "TXT",
        TYPE_NS => "NS",
        TYPE_SOA => "SOA",
        TYPE_HTTPS => "HTTPS",
        _ => "UNKNOWN",
    }
}

pub(crate) fn response_code_label(code: Option<u8>) -> String {
    match code {
        Some(RCODE_SUCCESS) => "NOERROR".into(),
        Some(RCODE_NAME_ERROR) => "NXDOMAIN".into(),
        Some(RCODE_SERVER_FAILURE) => "SERVFAIL".into(),
        Some(RCODE_REFUSED) => "REFUSED".into(),
        Some(value) => format!("RCODE {value}"),
        None => "INVALID".into(),
    }
}

/// Maximum number of answer records rendered into a history entry. Bounds the stored string
/// regardless of how large a response is (long CNAME chains, wildcard fan-out, etc.).
pub(crate) const HISTORY_MAX_ANSWERS: usize = 32;

/// Render the answer section of a DNS response into a compact, human-readable summary for the
/// query history (e.g. `"1.2.3.4, 5.6.7.8"`). Returns an empty string on unparseable or
/// answer-less responses. Callers on the hot cache path must not invoke this: the summary is
/// precomputed once at cache-insert time and reused from the cache entry.
pub(crate) fn summarize_answers(resp: &[u8]) -> Arc<str> {
    let Ok(msg) = DnsMessage::from_vec(resp) else {
        return Arc::from("");
    };
    if msg.answers.is_empty() {
        return Arc::from("");
    }
    let mut parts: Vec<String> = msg
        .answers
        .iter()
        .take(HISTORY_MAX_ANSWERS)
        .map(|record| format_hickory_rdata(&record.data))
        .collect();
    if msg.answers.len() > HISTORY_MAX_ANSWERS {
        parts.push("…".to_string());
    }
    Arc::from(parts.join(", "))
}

pub(crate) fn parse_lookup_records(resp: &[u8]) -> Result<Vec<DnsLookupRecord>> {
    let msg = DnsMessage::from_vec(resp).context("decode dns response")?;
    Ok(msg
        .answers
        .iter()
        .map(|record| {
            let rr_type: u16 = record.record_type().into();
            DnsLookupRecord {
                name: normalize_domain_lossy(&record.name.to_ascii()),
                record_type: qtype_label(rr_type).to_string(),
                ttl: record.ttl,
                value: format_hickory_rdata(&record.data),
            }
        })
        .collect())
}

pub(crate) fn format_hickory_rdata(data: &DnsRData) -> String {
    match data {
        DnsRData::SOA(soa) => {
            let mname = normalize_domain_lossy(&soa.mname.to_ascii());
            let rname = normalize_domain_lossy(&soa.rname.to_ascii());
            format!("{mname} {rname} serial={}", soa.serial)
        }
        _ => normalize_domain_lossy(&data.to_string()),
    }
}

pub(crate) fn response_for_query(req: &[u8], code: DnsResponseCode) -> DnsMessage {
    match DnsMessage::from_vec(req) {
        Ok(query) => {
            let mut response = DnsMessage::response(query.metadata.id, query.metadata.op_code);
            response.metadata.recursion_desired = query.metadata.recursion_desired;
            response.metadata.recursion_available = true;
            response.metadata.checking_disabled = query.metadata.checking_disabled;
            response.metadata.response_code = code;
            response.add_queries(query.queries);
            response
        }
        Err(_) => {
            let id = dns_message_id(req).unwrap_or(0);
            let mut response = DnsMessage::response(id, DnsOpCode::Query);
            response.metadata.response_code = code;
            response
        }
    }
}

pub(crate) fn response_for_question(question: &Question, code: DnsResponseCode) -> DnsMessage {
    let mut response = DnsMessage::response(question.id, question.op_code);
    response.metadata.recursion_desired = question.recursion_desired;
    response.metadata.recursion_available = true;
    response.metadata.checking_disabled = question.checking_disabled;
    response.metadata.response_code = code;
    response.add_query(question.query.clone());
    response
}

pub(crate) enum ResponseClass {
    Answer,
    Negative,
    Retry,
}

pub(crate) fn classify(resp: &[u8]) -> ResponseClass {
    // Negative responses deliberately mean "try the next upstream, but remember this
    // response as a fallback." This keeps ordered fallback semantics for domains that
    // only exist on a later upstream.
    match (rcode(resp), answer_count(resp)) {
        (Some(RCODE_SUCCESS), Some(count)) if count > 0 => ResponseClass::Answer,
        (Some(RCODE_SUCCESS), _) | (Some(RCODE_NAME_ERROR), _) => ResponseClass::Negative,
        _ => ResponseClass::Retry,
    }
}

pub(crate) fn rcode(resp: &[u8]) -> Option<u8> {
    (resp.len() >= 4).then(|| resp[3] & 0x0f)
}

pub(crate) fn answer_count(resp: &[u8]) -> Option<u16> {
    (resp.len() >= 8).then(|| u16::from_be_bytes([resp[6], resp[7]]))
}

pub(crate) fn answer_min_ttl(resp: &[u8]) -> Result<Option<u32>> {
    let ttl_offsets = section_ttl_offsets(resp, true)?;
    Ok(ttl_offsets
        .into_iter()
        .filter_map(|offset| {
            (offset + 4 <= resp.len()).then(|| {
                u32::from_be_bytes([
                    resp[offset],
                    resp[offset + 1],
                    resp[offset + 2],
                    resp[offset + 3],
                ])
            })
        })
        .min())
}

pub(crate) fn strip_aaaa_records(resp: &[u8]) -> Result<Vec<u8>> {
    let mut msg = DnsMessage::from_vec(resp).context("decode dns response")?;
    msg.answers
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.authorities
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.additionals
        .retain(|record| record.record_type() != DnsRecordType::AAAA);
    msg.to_vec().context("encode dns response")
}

pub(crate) fn skip_dns_name(msg: &[u8], mut offset: usize) -> Result<usize> {
    loop {
        let len = *msg
            .get(offset)
            .ok_or_else(|| anyhow!("dns name is truncated"))?;
        offset += 1;
        if len & 0b1100_0000 == 0b1100_0000 {
            if offset >= msg.len() {
                return Err(anyhow!("dns compression pointer is truncated"));
            }
            return Ok(offset + 1);
        }
        if len & 0b1100_0000 != 0 {
            return Err(anyhow!("unsupported dns name label type"));
        }
        if len == 0 {
            return Ok(offset);
        }
        offset += len as usize;
        if offset > msg.len() {
            return Err(anyhow!("dns name label is truncated"));
        }
    }
}

pub(crate) fn set_id(resp: &mut [u8], id: u16) {
    if resp.len() >= 2 {
        resp[0..2].copy_from_slice(&id.to_be_bytes());
    }
}

pub(crate) fn dns_message_id(msg: &[u8]) -> Option<u16> {
    (msg.len() >= 2).then(|| u16::from_be_bytes([msg[0], msg[1]]))
}

pub(crate) fn min_ttl_from_values(ttls: &[u32]) -> u32 {
    ttls.iter().copied().min().unwrap_or(0)
}

pub(crate) fn rr_ttls_with_offsets(resp: &[u8]) -> Result<(Vec<u32>, Vec<usize>)> {
    let ttl_offsets = rr_ttl_offsets(resp)?;
    let ttls = ttl_offsets
        .iter()
        .copied()
        .map(|offset| {
            if offset + 4 > resp.len() {
                return Err(anyhow!("dns rr ttl is truncated"));
            }
            Ok(u32::from_be_bytes([
                resp[offset],
                resp[offset + 1],
                resp[offset + 2],
                resp[offset + 3],
            ]))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((ttls, ttl_offsets))
}

#[cfg(test)]
pub(crate) fn rr_ttls(resp: &[u8]) -> Result<Vec<u32>> {
    rr_ttls_with_offsets(resp).map(|(ttls, _)| ttls)
}

pub(crate) fn rewrite_ttls(
    resp: &mut [u8],
    original_ttls: &[u32],
    ttl_offsets: &[usize],
    remaining: u32,
) -> Result<()> {
    if ttl_offsets.len() != original_ttls.len() {
        return Err(anyhow!("dns cached ttl count mismatch"));
    }
    for (offset, original) in ttl_offsets
        .iter()
        .copied()
        .zip(original_ttls.iter().copied())
    {
        if offset + 4 > resp.len() {
            return Err(anyhow!("dns rr ttl is truncated"));
        }
        let ttl = original.min(remaining);
        resp[offset..offset + 4].copy_from_slice(&ttl.to_be_bytes());
    }
    Ok(())
}

pub(crate) fn rr_ttl_offsets(resp: &[u8]) -> Result<Vec<usize>> {
    section_ttl_offsets(resp, false)
}

pub(crate) fn section_ttl_offsets(resp: &[u8], answers_only: bool) -> Result<Vec<usize>> {
    if resp.len() < 12 {
        return Err(anyhow!("dns response is too short"));
    }
    let qdcount = u16::from_be_bytes([resp[4], resp[5]]) as usize;
    let counts = [
        u16::from_be_bytes([resp[6], resp[7]]) as usize,
        u16::from_be_bytes([resp[8], resp[9]]) as usize,
        u16::from_be_bytes([resp[10], resp[11]]) as usize,
    ];
    let mut offset = 12;
    for _ in 0..qdcount {
        offset = skip_dns_name(resp, offset)?;
        if offset + 4 > resp.len() {
            return Err(anyhow!("dns question is truncated"));
        }
        offset += 4;
    }

    let mut ttl_offsets = Vec::new();
    for (section_index, count) in counts.into_iter().enumerate() {
        for _ in 0..count {
            offset = skip_dns_name(resp, offset)?;
            if offset + 10 > resp.len() {
                return Err(anyhow!("dns rr header is truncated"));
            }
            let ttl_offset = offset + 4;
            let rdlen = u16::from_be_bytes([resp[offset + 8], resp[offset + 9]]) as usize;
            offset += 10;
            if offset + rdlen > resp.len() {
                return Err(anyhow!("dns rr data is truncated"));
            }
            offset += rdlen;
            if !answers_only || section_index == 0 {
                ttl_offsets.push(ttl_offset);
            }
        }
    }
    if offset != resp.len() {
        return Err(anyhow!("dns response contains trailing bytes"));
    }
    Ok(ttl_offsets)
}

pub(crate) fn clamp_ttl(ttl: u32, min: u32, max: u32) -> u32 {
    let ttl = if min > 0 && ttl < min { min } else { ttl };
    if max > 0 && ttl > max {
        max
    } else {
        ttl
    }
}

pub(crate) fn normalize_domain(domain: &str) -> Result<String> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let ascii = idna::domain_to_ascii(trimmed).map_err(|_| anyhow!("invalid IDNA domain"))?;
    Ok(ascii.trim_end_matches('.').to_ascii_lowercase())
}

pub(crate) fn normalize_domain_lossy(domain: &str) -> String {
    normalize_domain(domain)
        .unwrap_or_else(|_| domain.trim().trim_end_matches('.').to_ascii_lowercase())
}
