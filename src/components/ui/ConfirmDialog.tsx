import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { t } from "../../lib/strings";
import "./confirm.css";

export interface ConfirmRequest {
  title: string;
  body?: string;
  okLabel: string;
  /** Red confirm button, for actions that delete something. */
  danger?: boolean;
  /** Shows a "No volver a mostrar" checkbox. */
  offerDontAsk?: boolean;
}

export interface ConfirmAnswer {
  ok: boolean;
  dontAsk: boolean;
}

type Confirm = (request: ConfirmRequest) => Promise<ConfirmAnswer>;

const ConfirmContext = createContext<Confirm | null>(null);

/** In-app replacement for the native alert, styled like the mini window's sheet. */
export function useConfirm(): Confirm {
  const confirm = useContext(ConfirmContext);
  if (!confirm) throw new Error("useConfirm needs a ConfirmProvider");
  return confirm;
}

interface Pending extends ConfirmRequest {
  resolve: (answer: ConfirmAnswer) => void;
}

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<Pending | null>(null);

  const confirm = useCallback<Confirm>(
    (request) =>
      new Promise<ConfirmAnswer>((resolve) => {
        setPending((previous) => {
          previous?.resolve({ ok: false, dontAsk: false }); // only one question at a time
          return { ...request, resolve };
        });
      }),
    [],
  );

  const answer = (ok: boolean, dontAsk: boolean) => {
    pending?.resolve({ ok, dontAsk });
    setPending(null);
  };

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      {pending && <ConfirmDialog request={pending} onAnswer={answer} />}
    </ConfirmContext.Provider>
  );
}

function ConfirmDialog({ request, onAnswer }: { request: ConfirmRequest; onAnswer: (ok: boolean, dontAsk: boolean) => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const action = useRef<HTMLButtonElement>(null);
  const [dontAsk, setDontAsk] = useState(false);

  // showModal() gives the focus trap, the inert background and Escape for free. Return always
  // confirms, in every dialog alike; Escape always cancels.
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
    action.current?.focus();
  }, []);

  return (
    <dialog
      ref={ref}
      className="confirm"
      role="alertdialog"
      aria-labelledby="confirm-title"
      aria-describedby={request.body ? "confirm-body" : undefined}
      onCancel={(e) => {
        e.preventDefault();
        onAnswer(false, false);
      }}
    >
      <h2 id="confirm-title">{request.title}</h2>
      {request.body && <p id="confirm-body">{request.body}</p>}
      <div className="confirm__actions">
        <button type="button" className="btn" onClick={() => onAnswer(false, false)}>
          {t.session.confirmCancel}
        </button>
        <button
          type="button"
          className={request.danger ? "btn confirm__danger" : "btn btn--signal"}
          ref={action}
          onClick={() => onAnswer(true, dontAsk)}
        >
          {request.okLabel}
        </button>
      </div>
      {request.offerDontAsk && (
        <label className="confirm__check">
          <input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk(e.target.checked)} />
          {t.mini.dontAskAgain}
        </label>
      )}
    </dialog>
  );
}
