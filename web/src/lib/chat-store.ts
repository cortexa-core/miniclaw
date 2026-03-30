// Chat state that persists across page navigation.
// Lives in a module-level store so it survives component mount/unmount.

export interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
  timestamp: Date;
  tools?: Array<{ name: string; status: string; durationMs?: number }>;
  totalMs?: number;
}

// Module-level state — survives component destruction
let _messages: ChatMessage[] = [];
let _sessionId: string = localStorage.getItem('uniclaw-session') || 'web';

export function getMessages(): ChatMessage[] {
  return _messages;
}

export function addMessage(msg: ChatMessage): number {
  _messages.push(msg);
  return _messages.length - 1;
}

export function updateMessageContent(idx: number, content: string) {
  if (_messages[idx]) {
    _messages[idx].content = content;
  }
}

export function appendMessageContent(idx: number, chunk: string) {
  if (_messages[idx]) {
    _messages[idx].content += chunk;
  }
}

export function getSessionId(): string {
  return _sessionId;
}

export function setSessionId(id: string) {
  _sessionId = id;
  localStorage.setItem('uniclaw-session', id);
}
