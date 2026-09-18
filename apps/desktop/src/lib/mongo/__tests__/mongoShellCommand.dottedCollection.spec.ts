import { describe, expect, it } from "vitest";
import { parseMongoCommand, splitMongoCommandRanges } from "@/lib/mongo/mongoShellCommand";

describe("MongoDB dotted collection names", () => {
  it("parses the profiler query after a use command", () => {
    const commands = splitMongoCommandRanges(
      "use yibai_statistics_report_center;\n" +
        "db.system.profile.find({ millis: { $gt: 100 } }).sort({ ts: -1 }).limit(20);",
    );

    expect(commands).toHaveLength(2);
    expect(commands[0]?.command).toMatchObject({ kind: "use", database: "yibai_statistics_report_center" });
    expect(commands[1]?.command).toMatchObject({
      kind: "find",
      collection: "system.profile",
      filter: '{ "millis": { "$gt": 100 } }',
      sort: '{ "ts": -1 }',
      limit: 20,
    });
  });

  it("preserves simple and getCollection syntax", () => {
    expect(parseMongoCommand("db.users.find({})")?.command).toMatchObject({ kind: "find", collection: "users" });
    expect(parseMongoCommand('db.getCollection("system.profile").find({})')?.command).toMatchObject({ kind: "find", collection: "system.profile" });
  });
});
