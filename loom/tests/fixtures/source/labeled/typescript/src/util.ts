import { format } from "date-fns";

export const MAX_ITEMS = 50;

function clean(part: string): string {
  return part.trim();
}

export function slugify(text: string): string {
  function clean(part: string): string {
    return part.trim().toLowerCase();
  }
  return clean(text);
}

export function headline(text: string): string {
  return clean(text);
}

export const parseCount = (raw: string): number => Number(raw);

export function stamp(date: Date): string {
  return format(date, "yyyy-MM-dd");
}
