// Writes THIRD-PARTY-NOTICES.txt: every library built into Pillbox, its licence, and the
// licence texts its authors ship. Run from the repository root after the dependencies change:
//   cargo metadata --format-version 1 > target\metadata.json
//   node installer\notices.js target\metadata.json
const fs = require('fs');
const path = require('path');

const meta = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
// Everything the workspace really builds with (no dev-only dependencies).
const used = new Set();
const stack = [...meta.workspace_members];
while (stack.length) {
  const id = stack.pop();
  if (used.has(id)) continue;
  used.add(id);
  for (const dep of nodes.get(id).deps) {
    if (dep.dep_kinds.some((k) => k.kind !== 'dev')) stack.push(dep.pkg);
  }
}
const packages = meta.packages
  .filter((p) => used.has(p.id) && !meta.workspace_members.includes(p.id))
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const LICENCE_FILE = /^(licen[cs]e|copying|notice)/i;
const out = [];
// Licence names of libraries that ship no licence file: their full texts go at the end.
const missing = new Set();
out.push('THIRD-PARTY NOTICES FOR PILLBOX');
out.push('');
out.push('Pillbox is built with the open-source libraries listed below. Each is used under its');
out.push('own licence, all of which allow free and commercial use. Their licence texts follow.');
out.push('');
out.push('Also used, not built in:');
out.push('- WebView2Loader.dll (Microsoft), redistributed under the Microsoft WebView2 SDK licence.');
out.push('- Weather, air quality and place search: Open-Meteo.com, data under CC BY 4.0.');
out.push('- Currency rates: Frankfurter (frankfurter.app), data from the European Central Bank.');
out.push('- Lyrics (only when switched on): LRCLIB (lrclib.net).');
out.push('- The installer is made with Inno Setup (jrsoftware.org), free for any use.');
out.push('');
out.push('='.repeat(78));
out.push('');
for (const p of packages) {
  out.push(`${p.name} ${p.version}`);
  out.push(`  Licence: ${p.license || 'see the licence file below'}`);
  if (p.repository) out.push(`  Source: ${p.repository}`);
  const dir = path.dirname(p.manifest_path);
  let files = [];
  try {
    files = fs.readdirSync(dir).filter((f) => LICENCE_FILE.test(f)).sort();
  } catch {
    /* not unpacked: the licence name above stands */
  }
  // Bundled fonts keep their licences beside them (egui's default fonts).
  try {
    for (const f of fs.readdirSync(path.join(dir, 'fonts'))) {
      if (f.endsWith('.txt')) files.push(path.join('fonts', f));
    }
  } catch {
    /* no fonts folder */
  }
  if (files.length === 0) {
    missing.add(p.license || '');
  }
  if (p.license_file && !files.includes(path.basename(p.license_file))) {
    files.push(path.basename(p.license_file));
  }
  for (const f of files) {
    let text;
    try {
      text = fs.readFileSync(path.join(dir, f), 'utf8').replace(/\r\n/g, '\n').trim();
    } catch {
      continue;
    }
    out.push('');
    out.push(`  --- ${f} ---`);
    out.push(text);
  }
  out.push('');
  out.push('-'.repeat(78));
  out.push('');
}
// The standard texts for the libraries above that name a licence but ship no file of it.
// Copied once from libraries that do ship them (found in the same Cargo registry).
const registry = path.dirname(path.dirname(packages[0].manifest_path));
const sample = (crate, file) => {
  const dir = fs.readdirSync(registry).find((d) => d.startsWith(crate + '-'));
  return dir
    ? fs.readFileSync(path.join(registry, dir, file), 'utf8').replace(/\r\n/g, '\n').trim()
    : null;
};
const wanted = [...missing].join(' ');
const standard = [
  ['MIT', 'anyhow', 'LICENSE-MIT'],
  ['Apache-2.0', 'anyhow', 'LICENSE-APACHE'],
  ['BSL-1.0', 'error-code', 'LICENSE'],
];
out.push('STANDARD LICENCE TEXTS');
out.push('For the libraries above that state a licence but do not ship its text. Each such library');
out.push('is Copyright (c) its authors, as listed on its source page.');
out.push('');
for (const [name, crate, file] of standard) {
  if (!wanted.includes(name)) continue;
  const text = sample(crate, file);
  if (!text) continue;
  out.push(`--- ${name} ---`);
  out.push(text);
  out.push('');
}
fs.writeFileSync('THIRD-PARTY-NOTICES.txt', out.join('\n') + '\n');
console.log(`${packages.length} libraries written to THIRD-PARTY-NOTICES.txt`);
