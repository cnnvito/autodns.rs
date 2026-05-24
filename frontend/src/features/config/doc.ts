import type { ConfigDocument } from "../../shared/types";

export type ConfigPageProps = {
  doc: ConfigDocument | null;
  onChange: (doc: ConfigDocument) => void;
};
