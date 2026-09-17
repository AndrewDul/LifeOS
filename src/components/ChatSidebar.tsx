export interface ChatListItem {
  id: string;
  title: string;
  preview: string;
}

interface ChatSidebarProps {
  chats: ChatListItem[];
  activeChatId: string;
  onSelectChat: (id: string) => void;
  onNewChat: () => void;
}

/**
 * Chat's own dedicated sidebar (build-start visual-refinement brief §4) -
 * New Chat / recent chats, ChatGPT-style. This is the only sidebar in
 * the app - there is no global section-navigation rail anymore (that was
 * removed from ScreenShell; the NeXa Island is the sole persistent
 * global navigation element). No AI, no persistence: selecting a chat
 * only changes which demo transcript ChatScreen shows.
 */
export function ChatSidebar({ chats, activeChatId, onSelectChat, onNewChat }: ChatSidebarProps) {
  return (
    <aside className="chat-sidebar" aria-label="Chats">
      <button type="button" className="chat-sidebar-new" onClick={onNewChat}>
        + New Chat
      </button>

      <input
        type="text"
        className="chat-sidebar-search"
        placeholder="Search chats..."
        disabled
      />

      <div className="chat-sidebar-list">
        {chats.map((chat) => (
          <button
            key={chat.id}
            type="button"
            className={`chat-sidebar-item${
              chat.id === activeChatId ? " chat-sidebar-item--active" : ""
            }`}
            onClick={() => onSelectChat(chat.id)}
          >
            <span className="chat-sidebar-item-title">{chat.title}</span>
            <span className="chat-sidebar-item-preview">{chat.preview}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}
