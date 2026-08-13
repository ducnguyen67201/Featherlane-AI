import type { PolicyImportStatus } from "./types";

export const MAX_POLICY_SOURCE_BYTES = 25 * 1024 * 1024;

const ACCEPTED_EXTENSIONS = [".pdf", ".docx", ".txt"];

export function validatePolicySourceFile(file: Pick<File, "name" | "size">): string | null {
  if (file.size === 0) return "The selected file is empty.";
  if (file.size > MAX_POLICY_SOURCE_BYTES) return "Policy sources must be 25 MiB or smaller.";
  const normalized = file.name.toLowerCase();
  if (!ACCEPTED_EXTENSIONS.some((extension) => normalized.endsWith(extension))) {
    return "Choose a PDF, DOCX, or UTF-8 TXT file.";
  }
  return null;
}

export function isActiveImportStatus(status: PolicyImportStatus) {
  return ["uploading", "queued", "parsing", "extracting"].includes(status);
}

export function importProgress(status: PolicyImportStatus) {
  const positions: Record<PolicyImportStatus, number> = {
    uploading: 8,
    queued: 20,
    parsing: 42,
    extracting: 72,
    review_required: 100,
    ready_to_compile: 100,
    compiled: 100,
    needs_ocr: 100,
    failed_retryable: 100,
    failed_terminal: 100,
  };
  return positions[status];
}
