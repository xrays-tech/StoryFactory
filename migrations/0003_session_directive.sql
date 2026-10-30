-- 0003_session_directive.sql — 会话全局指令（§6.3 CreateSandboxSessionReq.user_directive）
ALTER TABLE sandbox_sessions ADD COLUMN user_directive TEXT;
