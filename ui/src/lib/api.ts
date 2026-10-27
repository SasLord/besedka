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

interface RoomsResponse {
  rooms: Room[];
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
