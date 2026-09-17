// The reading pane renders sanitised email HTML inside a sandboxed srcdoc
// iframe. The only script allowed in there is this one: it turns link
// clicks into a postMessage the parent forwards to the system browser, and
// reports the document height so the frame can size itself.
//
// srcdoc frames inherit the app CSP, so this exact text is hashed into
// `script-src` in src-tauri/tauri.conf.json. If you change the script, run
// `node scripts/csp-hash.mjs` and update the hash there too.
export const FRAME_SCRIPT =
  "document.addEventListener('click',function(e){var t=e.target;var a=t&&t.closest?t.closest('a[href]'):null;if(!a)return;e.preventDefault();parent.postMessage({type:'tc-open',href:a.href},'*');});function h(){parent.postMessage({type:'tc-height',height:document.documentElement.scrollHeight},'*');}window.addEventListener('load',h);if(window.ResizeObserver){new ResizeObserver(h).observe(document.body);}setTimeout(h,50);setTimeout(h,500);";

export function buildFrameDoc(bodyHtml: string, allowRemoteImages: boolean): string {
  const img = allowRemoteImages ? "img-src * data: cid:" : "img-src data: cid:";
  return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; ${img}; style-src 'unsafe-inline'; script-src 'unsafe-inline'; font-src data:">
<style>
  html,body{margin:0;padding:0}
  body{font:14px/1.5 -apple-system,'Segoe UI',Roboto,sans-serif;color:#1f2328;padding:16px;word-wrap:break-word;overflow-wrap:anywhere}
  img{max-width:100%;height:auto}
  a{color:#0969da}
  pre{white-space:pre-wrap}
  blockquote{margin:0 0 0 .8ex;border-left:1px solid #ccc;padding-left:1ex;color:#555}
  table{max-width:100%}
</style></head><body>${bodyHtml}<script>${FRAME_SCRIPT}</script></body></html>`;
}
