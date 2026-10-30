import { PageHeader } from "../components/common/PageHeader";
import { RecycleBinPanel } from "../components/settings/RecycleBinPanel";
import { zh } from "../copy/zh";

export function RecyclePage() {
  return (
    <div className="space-y-5">
      <PageHeader eyebrow="数据安全" title={zh.recycle.title} description={zh.recycle.subtitle} />
      <RecycleBinPanel />
    </div>
  );
}
