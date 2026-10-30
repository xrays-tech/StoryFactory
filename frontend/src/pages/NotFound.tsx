import { useNavigate } from "react-router-dom";
import { Compass } from "lucide-react";
import { EmptyState } from "../components/common/EmptyState";
import { Button } from "../components/common/Button";

export function NotFoundPage() {
  const navigate = useNavigate();
  return (
    <div className="mx-auto max-w-2xl py-16">
      <EmptyState
        icon={<Compass className="h-5 w-5" />}
        title="这条路径没有故事"
        description="地址可能写错了，或这个页面已经被移动。回到项目书架继续吧。"
        action={
          <Button variant="primary" onClick={() => navigate("/")}>
            返回项目
          </Button>
        }
      />
    </div>
  );
}
