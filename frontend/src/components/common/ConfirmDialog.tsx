import { useUiStore } from "../../stores/uiStore";
import { Button } from "./Button";
import { Dialog } from "./Dialog";

export function ConfirmDialog() {
  const state = useUiStore((s) => s.confirmState);
  const resolve = useUiStore((s) => s.resolveConfirm);

  return (
    <Dialog
      open={!!state}
      onClose={() => resolve(false)}
      title={state?.title ?? ""}
      description={state?.message}
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={() => resolve(false)}>
            {state?.cancelLabel ?? "取消"}
          </Button>
          <Button
            variant={state?.tone === "danger" ? "danger" : "primary"}
            onClick={() => resolve(true)}
            data-autofocus
          >
            {state?.confirmLabel ?? "确认"}
          </Button>
        </>
      }
    />
  );
}
