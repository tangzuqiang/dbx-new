import type { ConnectionConfig, SidebarLayout } from "@/types/database";

export const UNGROUPED_WORKSPACE_LABEL = "未分组";

export function instanceWorkspaceTitle(connection: Pick<ConnectionConfig, "id" | "name">, layout: SidebarLayout): string {
  const groupId = findWorkspaceGroupId(layout.order, connection.id);
  const groupName = groupId ? layout.groups.find((group) => group.id === groupId)?.name : undefined;
  return `${groupName?.trim() || UNGROUPED_WORKSPACE_LABEL}-${connection.name}`;
}

export function findWorkspaceGroupId(entries: SidebarLayout["order"], connectionId: string, parentGroupId: string | null = null): string | null {
  for (const entry of entries) {
    if (entry.type === "connection" && entry.id === connectionId) return parentGroupId;
    if (entry.type !== "group") continue;
    if (entry.connectionIds?.includes(connectionId)) return entry.id;
    const nested = findWorkspaceGroupId(entry.children ?? [], connectionId, entry.id);
    if (nested) return nested;
  }
  return null;
}

export function appendUniqueWorkspace(ids: readonly string[], connectionId: string): string[] {
  return ids.includes(connectionId) ? [...ids] : [...ids, connectionId];
}
