import { invoke } from "@tauri-apps/api/core";

export interface CalendarEvent {
  readonly uid: string;
  readonly summary: string;
  readonly location: string;
  readonly startsAt: number;
  readonly endsAt: number;
  readonly allDay: boolean;
}

export interface CalendarLoad {
  readonly events: readonly CalendarEvent[];
  readonly errors: readonly string[];
  readonly status: string;
}

export function refreshCalendar(path: string) {
  return invoke<CalendarLoad>("refresh_calendar", { path });
}
