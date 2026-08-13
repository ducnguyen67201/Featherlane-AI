import Link from "next/link";
import { FlaskConical } from "lucide-react";

export function RunEvaluationButton() {
  return <Link className="primary-button" href="/agents"><FlaskConical size={16} />Choose target</Link>;
}
