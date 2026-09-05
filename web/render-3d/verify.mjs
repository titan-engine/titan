import init, { verify_three_d } from './pkg/three_d_browser.js';

const status = document.querySelector('#status');
const backend = new URL(location.href).searchParams.get('backend') ?? 'webgpu';
let timer;
try {
  if (!['webgpu', 'webgl2'].includes(backend)) throw new Error('Select webgpu or webgl2.');
  status.textContent = `Running ${backend}; 60-second deadline…`;
  const report = await Promise.race([
    (async () => { await init(); return JSON.parse(await verify_three_d(backend, document.createElement('canvas'))); })(),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('GPU verification exceeded 60 seconds; reload to discard this session.')), 60_000); }),
  ]);
  const download = document.querySelector('#download');
  download.href = URL.createObjectURL(new Blob([JSON.stringify(report)], { type: 'application/json' }));
  download.download = `three-d-${backend}-evidence.json`;
  download.hidden = false;
  const failures = report.evidence.images.flatMap(image => image.probes.filter(probe => !probe.passed).map(probe => `${image.name}/${image.format}: ${probe.name}, error ${probe.maximum_error}`));
  status.textContent = `${report.evidence.passed ? "PASS" : "FAIL"} ${backend}\n${report.adapter}\n${report.evidence.images.length} image cases; channel tolerance ${report.evidence.tolerance}.\n${report.evidence.edge_policy}\n${report.evidence.lifecycle_checks.join("\n")}\nCapture responses: ${JSON.stringify(report.evidence.capture_responses?.map(response => ({request_id:response.request_id,instance_id:response.instance_id,observed_frame:response.observed_frame,state_revision:response.state_revision,outcome:response.outcome?.status,identity:response.outcome?.response?.identity})))}\n${failures.join("\n")}`;
  for (const item of report.evidence.images ?? []) {
    const figure = document.createElement('figure');
    const canvas = document.createElement('canvas');
    canvas.width = item.width; canvas.height = item.height;
    canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(item.actual), item.width, item.height), 0, 0);
    const caption = document.createElement('figcaption');
    caption.textContent = item.name;
    figure.append(canvas, caption);
    document.querySelector('#images').append(figure);
  }
} catch (error) {
  status.textContent = `FAIL ${backend}: ${error?.message ?? error}`;
} finally {
  clearTimeout(timer);
}
