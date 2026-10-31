/** Besedka API client (Wave 0 + 1.1/1.2/1.3: auth, rooms, membership, SSO). */

export type Role = "user" | "manager" | "admin";

export interface User {
  id: number;
  username: string;
  display_name: string | null;
  role: Role;
  created_at: string;
}

export interface Room {
  id: number;
  kind: "general" | "room" | "channel" | "direct";
  title: string;
  created_at: string;
  /** Whether the current viewer is a member (Wave 1.3). */
  joined: boolean;
  /** Total members of the room (Wave 1.3). */
  member_count: number;
}

/** Public feature probe of the AD/SSO subsystem (no secrets, Wave 1.2). */
export interface SsoStatus {
  enabled: boolean;
  mock: boolean;
  sso: boolean;
  password_login: boolean;
}

/** A chat message (Wave 2.1). `created_at` is RFC 3339 UTC, milliseconds.
 *  `edited_at` (Wave 2.3) is set by the server on every edit — null while
 *  the message has never been edited. */
export interface Message {
  id: number;
  room_id: number;
  sender_id: number;
  text: string;
  created_at: string;
  edited_at?: string | null;
}

/** A user from the catalog endpoint (Wave 2.2) — enough to name senders. */
export interface UserRef {
  id: number;
  display_name: string;
}

interface RoomsResponse {
  rooms: Room[];
}

interface MessagesResponse {
  messages: Message[];
}

interface MessageEnvelope {
  message: Message;
}

interface UsersResponse {
  users: UserRef[];
}

interface RoomEnvelope {
  room: Room;
}

interface UserEnvelope {
  user: User;
}

/** A failed API call with the HTTP status and the server's error message. */
export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, init);
  if (!res.ok) {
    let message = `HTTP ${res.status}`;
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) message = body.error;
    } catch {
      /* non-JSON error body — keep the generic message */
    }
    throw new ApiError(res.status, message);
  }
  // 204 No Content (e.g. DELETE) has an empty body — nothing to parse.
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

function jsonInit(method: string, body?: unknown): RequestInit {
  return {
    method,
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  };
}

// ----- Wave 0 ----

export function fetchPing(): Promise<{ status: string; app: string; version: string }> {
  return request("/api/v1/ping");
}

/** The room list with each room's `joined` flag and `member_count`. */
export function fetchRooms(): Promise<Room[]> {
  return request<RoomsResponse>("/api/v1/rooms").then((r) => r.rooms);
}

// ----- Wave 1.1: local password auth ----

export function register(username: string, password: string): Promise<User> {
  return request<UserEnvelope>("/api/v1/register", jsonInit("POST", { username, password })).then(
    (r) => r.user,
  );
}

export function login(username: string, password: string): Promise<User> {
  return request<UserEnvelope>("/api/v1/login", jsonInit("POST", { username, password })).then(
    (r) => r.user,
  );
}

export function logout(): Promise<void> {
  return request("/api/v1/logout", jsonInit("POST")).then(() => undefined);
}

/** The currently authenticated user; 401 (`ApiError`) when anonymous. */
export function me(): Promise<User> {
  return request<UserEnvelope>("/api/v1/me").then((r) => r.user);
}

// ----- Wave 1.3: room membership ----

/** Join a room (idempotent). Returns the room with fresh membership state. */
export function joinRoom(id: number): Promise<Room> {
  return request<RoomEnvelope>(`/api/v1/rooms/${id}/join`, jsonInit("POST")).then((r) => r.room);
}

/** Leave a room; 404 when the room is unknown or the user was not a member. */
export function leaveRoom(id: number): Promise<Room> {
  return request<RoomEnvelope>(`/api/v1/rooms/${id}/leave`, jsonInit("POST")).then((r) => r.room);
}

// ----- Wave 2.2: chat + user catalog ----

/**
 * The latest `limit` messages (default 50, max 200) in ASCENDING order
 * (oldest → newest); with `beforeId` — the page immediately before that
 * message id (older page). Members only (403 otherwise).
 */
export function listMessages(roomId: number, limit?: number, beforeId?: number): Promise<Message[]> {
  const params = new URLSearchParams();
  if (limit !== undefined) params.set("limit", String(limit));
  if (beforeId !== undefined) params.set("before_id", String(beforeId));
  const suffix = params.size ? `?${params.toString()}` : "";
  return request<MessagesResponse>(`/api/v1/rooms/${roomId}/messages${suffix}`).then((r) => r.messages);
}

/** Post a message to the room (members only). Returns the stored message. */
export function postMessage(roomId: number, text: string): Promise<Message> {
  return request<MessageEnvelope>(`/api/v1/rooms/${roomId}/messages`, jsonInit("POST", { text })).then(
    (r) => r.message,
  );
}

// ----- Wave 2.3: edit + delete own messages ----

/** Edit the text of one of YOUR OWN messages (403 otherwise). Same
 *  validation as a new message (1–4096 chars after trimming). Returns the
 *  updated message with a fresh `edited_at` stamp. */
export function editMessage(roomId: number, messageId: number, text: string): Promise<Message> {
  return request<MessageEnvelope>(
    `/api/v1/rooms/${roomId}/messages/${messageId}`,
    jsonInit("PATCH", { text }),
  ).then((r) => r.message);
}

/** Delete one of YOUR OWN messages (403 otherwise). 204, empty body. */
export function deleteMessage(roomId: number, messageId: number): Promise<void> {
  return request(`/api/v1/rooms/${roomId}/messages/${messageId}`, { method: "DELETE" }).then(
    () => undefined,
  );
}

// ----- Wave 2.3: read receipts ----

/** Mark the room as read (members only): the cursor jumps to the latest
 *  message (or 0 for an empty room). Returns the new cursor. */
export function markRoomRead(roomId: number): Promise<number> {
  return request<{ last_read_id: number }>(`/api/v1/rooms/${roomId}/read`, {
    method: "POST",
  }).then((r) => r.last_read_id);
}

/** Who has read a given message (members only): `{ id, display_name }` list. */
export function messageReaders(roomId: number, messageId: number): Promise<UserRef[]> {
  return request<UsersResponse>(`/api/v1/rooms/${roomId}/messages/${messageId}/readers`).then(
    (r) => r.users,
  );
}

/** The user catalog: `{ id, display_name }` for every registered user. */
export function fetchUsers(): Promise<UserRef[]> {
  return request<UsersResponse>("/api/v1/users").then((r) => r.users);
}

// ----- Wave 1.2: AD/SSO ----

export function ssoStatus(): Promise<SsoStatus> {
  return request<SsoStatus>("/api/v1/auth_ad_sso/status");
}

/** AD (Kerberos) SSO login. The endpoint answers with JSON `{ ok, mock, user }`
 * plus the session cookie: in mock mode it is a direct fixture login
 * (`login` picks the fixture, e.g. `us100`/`us200`/`us300`), in real mode the
 * browser transparently completes the Negotiate handshake. */
export async function ssoLogin(login?: string): Promise<User> {
  const url = login
    ? `/api/v1/auth_ad_sso?login=${encodeURIComponent(login)}`
    : "/api/v1/auth_ad_sso";
  const res = await fetch(url, { credentials: "include" });
  if (!res.ok) {
    let message = "Не удалось выполнить вход через AD";
    try {
      const body = (await res.json()) as { error?: string };
      if (body.error) message = body.error;
    } catch {
      /* non-JSON error body — keep the default message */
    }
    throw new ApiError(res.status, message);
  }
  const body = (await res.json()) as { ok?: boolean; user?: User };
  if (!body.ok || !body.user) {
    throw new ApiError(0, "Не удалось выполнить вход через AD");
  }
  return body.user;
}
