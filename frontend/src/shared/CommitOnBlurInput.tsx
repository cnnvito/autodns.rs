import { Input, type InputProps } from "antd";
import { useEffect, useState } from "react";

type CommitOnBlurInputProps = Omit<InputProps, "value" | "onChange" | "onBlur" | "onPressEnter"> & {
  value: string;
  onCommit: (value: string) => void;
};

export function CommitOnBlurInput({ value, onCommit, ...props }: CommitOnBlurInputProps) {
  const [draft, setDraft] = useState(value);
  const [focused, setFocused] = useState(false);

  useEffect(() => {
    if (!focused) {
      setDraft(value);
    }
  }, [focused, value]);

  function commit() {
    setFocused(false);
    if (draft !== value) {
      onCommit(draft);
    }
  }

  return (
    <Input
      {...props}
      value={draft}
      onFocus={() => setFocused(true)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onPressEnter={(event) => event.currentTarget.blur()}
    />
  );
}
