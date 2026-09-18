import { describe, expect, it } from "vitest";
import type { ConnectionConfig, TreeNode } from "@/types/database";
import { applySidebarTableStorage, formatSidebarObjectStorage, sidebarTableStorageScopes, supportsSidebarTableStorage } from "../sidebarDatabaseStorage";

describe("sidebar object storage", () => {
  it("supports MongoDB collections and keeps their names unchanged", () => {
    const connection = { db_type: "mongodb" } as ConnectionConfig;
    const collection: TreeNode = { id: "c:db:orders", label: "orders", type: "mongo-collection", connectionId: "c", database: "db" };
    expect(supportsSidebarTableStorage(connection)).toBe(true);
    const scopes = sidebarTableStorageScopes([collection]);
    expect(scopes).toEqual([{ connectionId: "c", database: "db", schema: "" }]);
    expect(applySidebarTableStorage([collection], scopes[0], [{ name: "orders", total_bytes: 2 * 1024 ** 3 }])).toBe(true);
    expect(collection.label).toBe("orders");
    expect(collection.sizeBytes).toBe(2 * 1024 ** 3);
    expect(formatSidebarObjectStorage(collection.sizeBytes)).toBe("2 GB");
  });
});
