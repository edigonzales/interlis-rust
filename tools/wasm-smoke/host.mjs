const worker = new Worker('/worker.mjs', { type: 'module' });
let next = 0; const pending = new Map(); let failed = false;
window.hostReady = new Promise((resolve, reject) => {
  const timer = setTimeout(() => { failed = true; worker.terminate(); reject(new Error('Worker ready timeout')); }, 10_000);
  worker.onerror = (event) => {
    failed = true; clearTimeout(timer); reject(new Error(event.message || 'Worker failed'));
    for (const job of pending.values()) { clearTimeout(job.timer); job.reject(new Error('Worker failed')); }
    pending.clear(); worker.terminate();
  };
  worker.onmessage = ({ data }) => {
    if (data.type === 'ready') { clearTimeout(timer); resolve(data.version); return; }
    const job = pending.get(data.id); if (!job) return;
    clearTimeout(job.timer); pending.delete(data.id);
    if (data.fatal) { failed = true; worker.terminate(); }
    job.resolve(data);
  };
});
window.callWorker = async (request) => {
  await window.hostReady; if (failed) throw new Error('Worker is no longer usable');
  return new Promise((resolve, reject) => {
    const id = ++next;
    const timer = setTimeout(() => { failed = true; pending.delete(id); worker.terminate(); reject(new Error('Worker request timeout')); }, 10_000);
    pending.set(id, { resolve, reject, timer }); worker.postMessage({ id, request });
  });
};
