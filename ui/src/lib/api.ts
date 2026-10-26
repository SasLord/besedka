/** Besedka API client (Wave 0: ping + rooms). */

export interface PingResponse {
  status: string;
  app: string;
  version: string;
}

export interface Room {
  id: number;
  kind: "general" | "room" | "channel" | "direct";
  title: string;
  created_at: string;
}

interface RoomsResponse {
  rooms: Room[];
}

async function getJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
  return (await res.json()) as T;
}

export async function fetchPing(): Promise<PingResponse> {
  return getJson<PingResponse>("/api/v1/ping");
}

export async function fetchRooms(): Promise<Room[]> {
  const body = await getJson<RoomsResponse>("/api/v1/rooms");
  return body.rooms;
}
