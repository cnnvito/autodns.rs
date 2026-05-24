import * as Select from "@radix-ui/react-select";
import * as Switch from "@radix-ui/react-switch";
import { Check, ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

const emptyValue = "__autodns_empty__";

export type SelectOption = {
  value: string;
  label: string;
};

export function SelectField({
  value,
  options,
  onChange,
  placeholder = "请选择"
}: {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  placeholder?: string;
}) {
  const radixValue = value === "" ? emptyValue : value;

  return (
    <Select.Root value={radixValue} onValueChange={(next) => onChange(next === emptyValue ? "" : next)}>
      <Select.Trigger className="selectTrigger" aria-label={placeholder}>
        <Select.Value placeholder={placeholder} />
        <Select.Icon>
          <ChevronDown size={15} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content className="selectContent" position="popper" sideOffset={6}>
          <Select.Viewport>
            {options.map((option) => (
              <Select.Item className="selectItem" value={option.value === "" ? emptyValue : option.value} key={option.value || "__empty"}>
                <Select.ItemText>{option.label}</Select.ItemText>
                <Select.ItemIndicator className="selectItemIndicator">
                  <Check size={14} />
                </Select.ItemIndicator>
              </Select.Item>
            ))}
          </Select.Viewport>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}

export function SwitchField({
  checked,
  onChange,
  disabled = false,
  children
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <label className="switchField">
      <Switch.Root className="switchRoot" checked={checked} onCheckedChange={onChange} disabled={disabled}>
        <Switch.Thumb className="switchThumb" />
      </Switch.Root>
      <span>{children}</span>
    </label>
  );
}

export function NumberField({ label, value, onChange }: { label: string; value: number; onChange: (value: string) => void }) {
  return (
    <label className="compactField">
      <span>{label}</span>
      <input type="number" min="0" value={value} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}
