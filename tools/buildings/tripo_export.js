// Exports the open Tripo model as GLB to tripo_receiver.py from the built-in browser, which
// cannot download files. Paste this file's text into the Tripo tab once
// (`localStorage.__lookdevExport = <text>`), then on any model page:
//   eval(localStorage.__lookdevExport); await window.__exportTo('name.glb')
// It presses Tripo's Export twice (open, confirm), catches the GLB blob instead of the download,
// and posts it to the receiver; the form post navigates the tab to the receiver's answer.
window.__exportTo = async (name) => {
  window.__blobs = [];
  if (!window.__origCreate) {
    window.__origCreate = URL.createObjectURL;
    URL.createObjectURL = function (b) {
      if (b instanceof Blob) window.__blobs.push(b);
      return window.__origCreate.apply(this, arguments);
    };
  }
  if (!window.__clickHooked) {
    window.__clickHooked = true;
    const oc = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function () {
      if (this.download) { window.__lastDownload = this.download; return; }
      return oc.apply(this, arguments);
    };
  }
  const byText = (t) => [...document.querySelectorAll('button, div[role=button]')].filter((b) => b.innerText.trim() === t);
  for (let i = 0; i < 40 && !byText('Export').length; i++) await new Promise((r) => setTimeout(r, 500));
  const open = byText('Export');
  if (!open.length) return 'no export button';
  open[open.length - 1].click();
  await new Promise((r) => setTimeout(r, 1500));
  window.__blobs = [];
  const confirm = byText('Export');
  confirm[confirm.length - 1].click();
  const isGlb = async (b) => new Uint8Array(await b.slice(0, 4).arrayBuffer()).every((v, i) => v === [0x67, 0x6c, 0x54, 0x46][i]);
  let blob = null;
  for (let i = 0; i < 120 && !blob; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    for (const b of window.__blobs) if (await isGlb(b)) blob = b;
  }
  if (!blob) return 'no glb blob';
  const dt = new DataTransfer();
  dt.items.add(new File([blob], name, {type: 'model/gltf-binary'}));
  const form = document.createElement('form');
  form.method = 'POST';
  form.enctype = 'multipart/form-data';
  form.action = 'http://127.0.0.1:8766/upload';
  const input = document.createElement('input');
  input.type = 'file';
  input.name = 'file';
  input.files = dt.files;
  form.appendChild(input);
  document.body.appendChild(form);
  setTimeout(() => form.submit(), 50);
  return 'submitting ' + name + ' ' + blob.size;
};
