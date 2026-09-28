import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StageSummaryText } from "@/components/stage-detail";

describe("StageSummaryText", () => {
  it("shows the plan-authored summary, preserving line breaks", () => {
    const { container } = render(
      <StageSummaryText summary={"Wires the button.\nHandles the click."} />,
    );

    const paragraph = container.querySelector("p");
    expect(paragraph?.textContent).toBe("Wires the button.\nHandles the click.");
    expect(paragraph?.className).toContain("whitespace-pre-line");
  });

  it("makes the scrollable summary a focusable labelled region", () => {
    const { container } = render(<StageSummaryText summary="Wires the button." />);

    const region = container.querySelector('[role="region"]');
    expect(region?.getAttribute("aria-label")).toBe("stage summary");
    expect(region?.getAttribute("tabindex")).toBe("0");
  });

  it("renders no block when the stage has no summary", () => {
    const { container } = render(<StageSummaryText summary={null} />);

    expect(container.innerHTML).toBe("");
  });
});
