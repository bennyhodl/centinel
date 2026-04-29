import { EDITOR_INTRO_MESSAGE } from "@/lib/chat-prompt";
import ChatClient from "./_components/ChatClient";

export const dynamic = "force-dynamic";

export default function ChatPage() {
  return <ChatClient intro={EDITOR_INTRO_MESSAGE} />;
}
