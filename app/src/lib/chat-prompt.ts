/**
 * Lightweight intro for /chat.
 *
 * The /chat surface now spawns a real Hermes session per message, with
 * the `centinel-operator` skill loaded. The skill itself owns the
 * system prompt, citation rules, and CLI cheat sheet — see
 * `skills/centinel-operator/SKILL.md`. This file just owns the
 * empty-state copy.
 */

export const EDITOR_INTRO_MESSAGE = `Ask me anything about the wiki — I search QMD on every turn and cite sources. I can also run lifecycle commands: pause/resume/trigger investigations, sweep snoozed queue items, list cron jobs. Destructive actions wait for your explicit confirmation. For approving entity merges or promoting drafts, the /operator-queue and /findings UIs keep a cleaner audit trail than chat.`;

