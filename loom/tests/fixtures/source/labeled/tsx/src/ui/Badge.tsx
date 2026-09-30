import React from "react";

export interface BadgeProps {
  text: string;
  tone?: "info" | "warn";
}

export function Badge({ text, tone = "info" }: BadgeProps) {
  return <span className={`badge ${tone}`}>{text}</span>;
}
