import { describe, expect, it } from "vitest";
import { appendUniqueWorkspace, instanceWorkspaceTitle } from "@/lib/workspace/instanceWorkspace";

describe("instanceWorkspace", () => {
  const connection = { id: "prod", name: "主库" };

  it("uses the containing group and connection name", () => {
    expect(
      instanceWorkspaceTitle(connection, {
        groups: [{ id: "g1", name: "生产", collapsed: false }],
        order: [{ type: "group", id: "g1", connectionIds: ["prod"] }],
      }),
    ).toBe("生产-主库");
  });

  it("supports nested groups and ungrouped connections", () => {
    expect(
      instanceWorkspaceTitle(connection, {
        groups: [{ id: "child", name: "华东", collapsed: false }],
        order: [{ type: "group", id: "root", children: [{ type: "group", id: "child", connectionIds: ["prod"] }] }],
      }),
    ).toBe("华东-主库");
    expect(instanceWorkspaceTitle(connection, { groups: [], order: [{ type: "connection", id: "prod" }] })).toBe("未分组-主库");
  });

  it("does not create duplicate workspaces for one connection", () => {
    expect(appendUniqueWorkspace(["prod"], "prod")).toEqual(["prod"]);
  });
});
