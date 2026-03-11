import { describe, expect, it } from "vitest";

import { APP_SHELL } from "./app_shell";

describe("APP_SHELL", () => {
  it("renders the thread detail area with its own scroll shell", () => {
    expect(APP_SHELL).toContain('class="detail-scroll-shell"');
    expect(APP_SHELL).toContain('id="eventSearchInput"');
    expect(APP_SHELL).toContain('id="detailEventsList"');
  });
});
