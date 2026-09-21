import { useEffect, useRef } from "react";
import { Bold, Italic, Link as LinkIcon, List, ListOrdered, Quote, RemoveFormatting, Underline } from "lucide-react";

/**
 * A small contentEditable editor for the composer. Formatting goes through
 * `document.execCommand`, which both WebView2 and WebKitGTK still support
 * and which keeps the output plain HTML the sanitiser already accepts.
 */
export function RichEditor({
  html,
  onChange,
  placeholder,
  autoFocus,
}: {
  html: string;
  onChange: (html: string, text: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);

  // Only push external content in when it differs from what is on screen,
  // otherwise every keystroke would reset the caret.
  useEffect(() => {
    const el = ref.current;
    if (el && el.innerHTML !== html) el.innerHTML = html;
  }, [html]);
  useEffect(() => {
    if (autoFocus) ref.current?.focus();
  }, [autoFocus]);

  const emit = () => {
    const el = ref.current;
    if (!el) return;
    const empty = el.innerText.trim() === "" && !el.querySelector("img");
    onChange(empty ? "" : el.innerHTML, el.innerText.replace(/ /g, " "));
  };
  const cmd = (name: string, value?: string) => {
    ref.current?.focus();
    document.execCommand(name, false, value);
    emit();
  };
  const link = () => {
    const url = window.prompt("Link address", "https://");
    if (url && url !== "https://") cmd("createLink", url);
  };
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = e.key.toLowerCase();
    if (k === "b") (e.preventDefault(), cmd("bold"));
    else if (k === "i") (e.preventDefault(), cmd("italic"));
    else if (k === "u") (e.preventDefault(), cmd("underline"));
    else if (k === "k") (e.preventDefault(), link());
  };
  // Pasted rich content from other apps drags along fonts, colours and
  // tracking pixels; keep plain text and let the user format it here.
  const onPaste = (e: React.ClipboardEvent) => {
    e.preventDefault();
    const text = e.clipboardData.getData("text/plain");
    document.execCommand("insertText", false, text);
    emit();
  };

  return (
    <div className="flex min-h-[160px] flex-1 flex-col">
      <div className="flex items-center gap-0.5 border-b border-gray-100 px-2 py-1 text-gray-600">
        <Tool title="Bold (Ctrl+B)" onClick={() => cmd("bold")}>
          <Bold size={14} />
        </Tool>
        <Tool title="Italic (Ctrl+I)" onClick={() => cmd("italic")}>
          <Italic size={14} />
        </Tool>
        <Tool title="Underline (Ctrl+U)" onClick={() => cmd("underline")}>
          <Underline size={14} />
        </Tool>
        <span className="mx-1 h-4 w-px bg-gray-200" />
        <Tool title="Bulleted list" onClick={() => cmd("insertUnorderedList")}>
          <List size={14} />
        </Tool>
        <Tool title="Numbered list" onClick={() => cmd("insertOrderedList")}>
          <ListOrdered size={14} />
        </Tool>
        <Tool title="Quote" onClick={() => cmd("formatBlock", "blockquote")}>
          <Quote size={14} />
        </Tool>
        <Tool title="Link (Ctrl+K)" onClick={link}>
          <LinkIcon size={14} />
        </Tool>
        <span className="mx-1 h-4 w-px bg-gray-200" />
        <Tool title="Clear formatting" onClick={() => (cmd("removeFormat"), cmd("formatBlock", "div"))}>
          <RemoveFormatting size={14} />
        </Tool>
      </div>
      <div
        ref={ref}
        contentEditable
        suppressContentEditableWarning
        role="textbox"
        aria-multiline="true"
        aria-label="Message body"
        data-placeholder={placeholder}
        className="rich-editor flex-1 overflow-y-auto px-3 py-2 text-sm outline-none"
        onInput={emit}
        onBlur={emit}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
      />
    </div>
  );
}

function Tool({ title, onClick, children }: { title: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      className="rounded p-1 hover:bg-gray-100 hover:text-gray-900"
      title={title}
      // mousedown would steal the selection the command should apply to
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
