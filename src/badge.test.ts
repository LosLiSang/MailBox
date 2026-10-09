import { describe, expect, it } from "vitest";
import { formatBadgeLabel } from "./badge";

describe("formatBadgeLabel", () => {
  const cases: [count: number, expected: string][] = [
    [0, ""],
    [-1, ""],
    [-10, ""],
    [1, "1"],
    [9, "9"],
    [10, "10"],
    [99, "99"],
    [100, "99+"],
    [999, "99+"],
  ];

  it.each(cases)("count=%i -> %s", (count, expected) => {
    expect(formatBadgeLabel(count)).toBe(expected);
  });
});
