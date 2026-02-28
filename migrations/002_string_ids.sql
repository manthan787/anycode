-- Rename telegram_message_id to platform_message_id for platform-agnostic naming.
ALTER TABLE pending_interactions RENAME COLUMN telegram_message_id TO platform_message_id;
