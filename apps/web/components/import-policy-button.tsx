import Link from "next/link";
import { Plus } from "lucide-react";

export function ImportPolicyButton() {
  return (
    <div className="import-policy-action">
      <Link className="primary-button" href="/policies/imports/new">
        <Plus size={16} />
        Import policy source
      </Link>
      <span className="import-policy-status">PDF, DOCX, TXT, or pasted text</span>
    </div>
  );
}
