import Link from "next/link";
import { Plus } from "lucide-react";

export function ImportPolicyButton() {
  return (
    <div className="import-policy-action">
      <Link className="primary-button" href="/policies/collections/new">
        <Plus size={16} />
        Create policy collection
      </Link>
      <span className="import-policy-status">Files, paste, URL, Drive, Microsoft 365, or Notion</span>
    </div>
  );
}
