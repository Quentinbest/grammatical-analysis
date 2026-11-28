import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const errors = [];

function readText(relPath) {
  return fs.readFileSync(path.join(root, relPath), "utf8");
}

function checkVersionParity() {
  const pkg = JSON.parse(readText("package.json"));
  const tauriConf = JSON.parse(readText("src-tauri/tauri.conf.json"));
  const cargoToml = readText("src-tauri/Cargo.toml");
  const cargoMatch = /^version\s*=\s*"([^"]+)"/m.exec(cargoToml);

  const versions = {
    "package.json": pkg.version,
    "src-tauri/tauri.conf.json": tauriConf.version,
    "src-tauri/Cargo.toml": cargoMatch?.[1] ?? "<missing>",
  };

  const unique = new Set(Object.values(versions));
  if (unique.size !== 1) {
    const details = Object.entries(versions)
      .map(([file, ver]) => `${file}=${ver}`)
      .join(", ");
    errors.push(
      `Version mismatch across release manifests (${details}). Update all three files together.`,
    );
  }
}

function parseRustDefaultProviders(source) {
  const fnStart = source.indexOf("pub fn default_providers()");
  const fnEnd = source.indexOf("pub fn default_actions()");
  if (fnStart === -1 || fnEnd === -1 || fnEnd <= fnStart) {
    errors.push("Could not locate default_providers() in src-tauri/src/storage/mod.rs.");
    return new Map();
  }
  const block = source.slice(fnStart, fnEnd);
  const providers = new Map();
  const entryRe =
    /id:\s*"([^"]+)"\.into\(\)[\s\S]*?name:\s*"([^"]+)"\.into\(\)[\s\S]*?base_url:\s*"([^"]+)"\.into\(\)[\s\S]*?model:\s*"([^"]+)"\.into\(\)/g;
  for (const match of block.matchAll(entryRe)) {
    providers.set(match[1], { name: match[2], baseUrl: match[3], model: match[4] });
  }
  return providers;
}

function checkModelCatalogParity(rustProviders) {
  const catalogSrc = readText("src/domain/modelCatalog.ts");
  const presetsStart = catalogSrc.indexOf("export const MODEL_PRESETS");
  const presetsEnd = catalogSrc.indexOf("];", presetsStart);
  if (presetsStart === -1 || presetsEnd === -1) {
    errors.push("Could not locate MODEL_PRESETS in src/domain/modelCatalog.ts.");
    return;
  }
  const presetsBlock = catalogSrc.slice(presetsStart, presetsEnd);
  const presetRe =
    /id:\s*mustProviderId\("([^"]+)"\)[\s\S]*?name:\s*"([^"]+)"[\s\S]*?baseUrl:\s*"([^"]+)"[\s\S]*?model:\s*"([^"]+)"/g;
  const seenIds = new Set();
  for (const match of presetsBlock.matchAll(presetRe)) {
    const [, id, name, baseUrl, model] = match;
    seenIds.add(id);
    const rustEntry = rustProviders.get(id);
    if (!rustEntry) {
      errors.push(
        `modelCatalog MODEL_PRESETS id "${id}" is missing from default_providers() in src-tauri/src/storage/mod.rs.`,
      );
      continue;
    }
    if (rustEntry.name !== name || rustEntry.baseUrl !== baseUrl || rustEntry.model !== model) {
      errors.push(
        `modelCatalog preset "${id}" (${name}, ${baseUrl}, ${model}) drifted from src-tauri/src/storage/mod.rs (${rustEntry.name}, ${rustEntry.baseUrl}, ${rustEntry.model}).`,
      );
    }
  }
  for (const rustId of rustProviders.keys()) {
    if (!seenIds.has(rustId)) {
      errors.push(
        `Rust default_providers() id "${rustId}" is missing from MODEL_PRESETS in src/domain/modelCatalog.ts.`,
      );
    }
  }
}

function checkProviderPresetParity() {
  const rustProviders = parseRustDefaultProviders(readText("src-tauri/src/storage/mod.rs"));
  if (rustProviders.size === 0) {
    errors.push("Parsed 0 providers from default_providers() in src-tauri/src/storage/mod.rs.");
    return;
  }

  checkModelCatalogParity(rustProviders);

  const drawerSrc = readText("src/components/SettingsDrawer/SettingsDrawer.tsx");
  const presetsStart = drawerSrc.indexOf("const PROVIDER_PRESETS");
  const presetsEnd = drawerSrc.indexOf("];", presetsStart);
  if (presetsStart === -1 || presetsEnd === -1) {
    errors.push(
      "Could not locate PROVIDER_PRESETS in src/components/SettingsDrawer/SettingsDrawer.tsx.",
    );
    return;
  }
  const presetsBlock = drawerSrc.slice(presetsStart, presetsEnd);
  const presetRe =
    /id:\s*"([^"]+)"[\s\S]*?baseUrl:\s*"([^"]+)"[\s\S]*?model:\s*"([^"]+)"/g;
  let presetCount = 0;
  for (const match of presetsBlock.matchAll(presetRe)) {
    presetCount += 1;
    const [, id, baseUrl, model] = match;
    const rustEntry = rustProviders.get(id);
    if (!rustEntry) {
      errors.push(
        `SettingsDrawer PROVIDER_PRESETS id "${id}" is missing from default_providers() in src-tauri/src/storage/mod.rs.`,
      );
      continue;
    }
    if (rustEntry.baseUrl !== baseUrl || rustEntry.model !== model) {
      errors.push(
        `SettingsDrawer preset "${id}" (${baseUrl}, ${model}) drifted from src-tauri/src/storage/mod.rs (${rustEntry.baseUrl}, ${rustEntry.model}).`,
      );
    }
  }
  if (presetCount === 0) {
    errors.push("Parsed 0 entries from PROVIDER_PRESETS in SettingsDrawer.tsx.");
  }
}

checkVersionParity();
checkProviderPresetParity();

if (errors.length > 0) {
  for (const err of errors) {
    console.error(`check-repo FAIL: ${err}`);
  }
  process.exit(1);
}

console.log("check-repo ok: version manifests and provider presets are in sync.");
