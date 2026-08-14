import { validatePolicySourceFile } from "./policy-import";

export const MAX_POLICY_SOURCE_BATCH_BYTES = 100 * 1024 * 1024;
export const MAX_POLICY_SOURCE_BATCH_ITEMS = 25;

export function validatePolicySourceBatch(files: Array<Pick<File, "name" | "size">>) {
  if (files.length === 0) return "Choose at least one file.";
  if (files.length > MAX_POLICY_SOURCE_BATCH_ITEMS) return "A batch can contain at most 25 files.";
  const invalid = files.map(validatePolicySourceFile).find(Boolean);
  if (invalid) return invalid;
  if (files.reduce((total, file) => total + file.size, 0) > MAX_POLICY_SOURCE_BATCH_BYTES) {
    return "A batch can contain at most 100 MiB.";
  }
  return null;
}

export function validatePublicPolicyUrls(value: string) {
  const urls = value.split(/\r?\n/).map((url) => url.trim()).filter(Boolean);
  if (urls.length === 0 || urls.length > MAX_POLICY_SOURCE_BATCH_ITEMS) {
    return { error: "Enter 1–25 public HTTPS URLs, one per line.", urls: [] };
  }
  if (new Set(urls).size !== urls.length) {
    return { error: "Each public URL can appear only once per batch.", urls: [] };
  }
  try {
    if (urls.some((value) => {
      const url = new URL(value);
      return url.protocol !== "https:" || Boolean(url.username || url.password || url.hash);
    })) {
      return { error: "Enter public HTTPS URLs without credentials or fragments.", urls: [] };
    }
  } catch {
    return { error: "Enter valid public HTTPS URLs.", urls: [] };
  }
  return { error: null, urls };
}

export function validateProviderSelectionIds(
  provider: "google_drive" | "microsoft_graph" | "notion",
  value: string,
) {
  const ids = value.split(/\r?\n/).map((id) => id.trim()).filter(Boolean);
  if (ids.length === 0 || ids.length > MAX_POLICY_SOURCE_BATCH_ITEMS || new Set(ids).size !== ids.length) {
    return { error: "Enter 1–25 unique provider item IDs, one per line.", ids: [] };
  }
  const valid = ids.every((id) => {
    if (provider === "google_drive") return /^[a-zA-Z0-9_-]{10,256}$/.test(id);
    if (provider === "microsoft_graph") {
      const [driveId, itemId, extra] = id.split(":");
      return !extra && Boolean(driveId && itemId && driveId.length <= 256 && itemId.length <= 256);
    }
    return /^[a-fA-F0-9-]{32,36}$/.test(id);
  });
  return valid ? { error: null, ids } : { error: `One or more ${provider.replaceAll("_", " ")} item IDs are invalid.`, ids: [] };
}
