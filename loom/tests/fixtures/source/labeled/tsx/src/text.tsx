export function shorten(text: string, max: number): string {
  return text.length > max ? text.slice(0, max) : text;
}

export const Bold = ({ children }: { children: string }) => <b>{children}</b>;
