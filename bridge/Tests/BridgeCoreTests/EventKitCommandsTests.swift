import Foundation
import XCTest
@testable import BridgeCore

/// Row encoding, query helpers, and every `calendar.*` / `reminders.*` /
/// `contacts.*` command through the router against the fakes.
final class EventKitCommandsTests: XCTestCase {
    private var router: CommandRouter!
    private var eventKit: FakeEventKitService!
    private var contacts: FakeContactsService!

    override func setUp() async throws {
        eventKit = FakeEventKitService(
            calendars: Fixtures.calendars, events: Fixtures.events, lists: Fixtures.lists, reminders: Fixtures.reminders)
        contacts = FakeContactsService(rows: Fixtures.contacts)
        router = CommandRouter(version: "test")
        await registerEventKitCommands(router, service: eventKit)
        await registerContactsCommands(router, service: contacts)
    }

    private func call(_ cmd: String, _ args: [String: JSONValue] = [:]) async -> Response {
        await router.dispatch(Request(id: 1, cmd: cmd, args: args))
    }

    private func data<T: Decodable>(_ type: T.Type, _ cmd: String, _ args: [String: JSONValue] = [:],
                                    file: StaticString = #filePath, line: UInt = #line) async throws -> T {
        let response = await call(cmd, args)
        guard response.ok, let data = response.data else {
            XCTFail("\(cmd) failed: \(response.error?.code ?? "?") \(response.error?.message ?? "")", file: file, line: line)
            throw BridgeError(body: response.error ?? ErrorBody(code: "internal", message: "no data"))
        }
        return try JSONDecoder().decode(type, from: try JSONEncoder().encode(data))
    }

    private func expectError(_ cmd: String, _ args: [String: JSONValue] = [:], code: String,
                             messageContains fragment: String? = nil,
                             file: StaticString = #filePath, line: UInt = #line) async {
        let response = await call(cmd, args)
        XCTAssertFalse(response.ok, "\(cmd) unexpectedly succeeded", file: file, line: line)
        XCTAssertEqual(response.error?.code, code, response.error?.message ?? "", file: file, line: line)
        if let fragment {
            XCTAssertTrue(response.error?.message.localizedCaseInsensitiveContains(fragment) ?? false,
                          "'\(response.error?.message ?? "")' lacks '\(fragment)'", file: file, line: line)
        }
    }

    // MARK: Row encoding

    func testCalendarEventRowWire() throws {
        let row = Fixtures.events[1]
        let value = try JSONValue(encoding: row)
        XCTAssertEqual(value["id"], "ev-standup")
        XCTAssertEqual(value["calendar_id"], "cal-home")
        XCTAssertEqual(value["start"], .string(DateCoding.format(row.start!)))
        XCTAssertEqual(value["end"], .string(DateCoding.format(row.end!)))
        XCTAssertEqual(value["all_day"], false)
        XCTAssertEqual(value["has_recurrence"], true)
        XCTAssertEqual(value["attendee_count"], 3)
        XCTAssertEqual(value["alarm_count"], 1)
        XCTAssertEqual(value["modified_at"], .string(DateCoding.format(row.modifiedAt!)))
        XCTAssertEqual(value["url"], "https://example.com/standup")
        XCTAssertEqual(DateCoding.parse(value["start"]!.stringValue!), row.start)
        // Wire dates carry an offset, not Z.
        XCTAssertTrue(value["start"]!.stringValue!.wholeMatch(of: /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-]\d{2}:\d{2}/) != nil)

        // Absent fields are explicit nulls, and the row round-trips.
        let sparse = try JSONValue(encoding: Fixtures.events[2])
        XCTAssertEqual(sparse["location"], .null)
        XCTAssertEqual(sparse["notes"], .null)
        XCTAssertEqual(sparse["modified_at"], .null)
        XCTAssertEqual(sparse["created_at"], .null)
        XCTAssertEqual(sparse["all_day"], true)
        XCTAssertEqual(Set(sparse.objectValue!.keys), [
            "id", "title", "calendar", "calendar_id", "location", "start", "end", "all_day", "notes", "url",
            "modified_at", "created_at", "has_recurrence", "attendee_count", "alarm_count",
        ])
        let decoded = try JSONDecoder().decode(CalendarEventRow.self, from: try JSONEncoder().encode(row))
        XCTAssertEqual(decoded, row)
    }

    func testCalendarRowWire() throws {
        let value = try JSONValue(encoding: Fixtures.calendars)
        XCTAssertEqual(value[0], ["id": "cal-home", "title": "Home", "type": "caldav", "allows_modifications": true, "color": "#FF0000"])
        XCTAssertEqual(value[1]?["color"], .null)
        XCTAssertEqual(CalendarRow.hexColor(components: [1, 0.5, 0, 1]), "#FF8000")
        XCTAssertEqual(CalendarRow.hexColor(components: [0, 0, 1]), "#0000FF")
        XCTAssertNil(CalendarRow.hexColor(components: [0.5]))
        XCTAssertNil(CalendarRow.hexColor(components: nil))
    }

    func testReminderRowWire() throws {
        let value = try JSONValue(encoding: Fixtures.reminders[2])
        XCTAssertEqual(value["id"], "rem-call")
        XCTAssertEqual(value["list_id"], "list-todo")
        XCTAssertEqual(value["completed"], false)
        XCTAssertEqual(value["completion_date"], .null)
        XCTAssertEqual(value["due"], .string(DateCoding.format(Fixtures.reminders[2].due!)))
        XCTAssertEqual(value["due_all_day"], false)
        XCTAssertEqual(value["priority"], 1)
        XCTAssertEqual(value["url"], "https://example.com")
        XCTAssertEqual(Set(value.objectValue!.keys), [
            "id", "title", "list", "list_id", "notes", "completed", "completion_date", "due", "due_all_day", "priority",
            "modified_at", "created_at", "url",
        ])
        let milk = try JSONValue(encoding: Fixtures.reminders[0])
        XCTAssertEqual(milk["due_all_day"], true)
        XCTAssertEqual(milk["notes"], .null)
        let lists = try JSONValue(encoding: Fixtures.lists)
        XCTAssertEqual(lists[1], ["id": "list-shop", "title": "Shopping", "allows_modifications": true, "color": nil])
        XCTAssertEqual(try JSONDecoder().decode(ReminderRow.self, from: try JSONEncoder().encode(Fixtures.reminders[2])),
                       Fixtures.reminders[2])
    }

    func testContactRowWire() throws {
        let value = try JSONValue(encoding: Fixtures.contacts[0])
        XCTAssertEqual(value["given_name"], "Paul")
        XCTAssertEqual(value["family_name"], "Thrasher")
        XCTAssertEqual(value["emails"], [["label": "home", "value": "paul@example.com"]])
        XCTAssertEqual(value["phones"], [["label": nil, "value": "+1 555 0100"]])
        XCTAssertEqual(value["birthday"], "--05-04")
        XCTAssertEqual(value["modified_at"], .null)
        XCTAssertEqual(value["nickname"], .null)
        let sparse = try JSONValue(encoding: Fixtures.contacts[2])
        XCTAssertEqual(sparse["family_name"], .null)
        XCTAssertEqual(sparse["emails"], [])
        XCTAssertEqual(ContactRow.birthdayString(year: 1990, month: 5, day: 4), "1990-05-04")
        XCTAssertEqual(ContactRow.birthdayString(year: nil, month: 12, day: 25), "--12-25")
        XCTAssertNil(ContactRow.birthdayString(year: 1990, month: nil, day: 4))
        XCTAssertNil(ContactRow.nonEmpty(""))
        XCTAssertEqual(ContactRow.nonEmpty("x"), "x")
    }

    // MARK: Query helpers

    func testDueDateParsing() throws {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = Fixtures.tz

        let day = try DueDate.parse("2026-09-05", calendar: cal)
        XCTAssertFalse(day.hasTime)
        XCTAssertEqual(day.components.year, 2026)
        XCTAssertEqual(day.components.day, 5)
        XCTAssertNil(day.components.hour)
        XCTAssertEqual(day.date(calendar: cal), DateCoding.parse("2026-09-05T00:00:00-07:00"))

        let timed = try DueDate.parse("2026-09-04T15:30:00-07:00", calendar: cal)
        XCTAssertTrue(timed.hasTime)
        XCTAssertEqual(timed.components.hour, 15)
        XCTAssertEqual(timed.components.minute, 30)
        XCTAssertEqual(timed.date(calendar: cal), DateCoding.parse("2026-09-04T15:30:00-07:00"))
        // Timed components carry their zone, so the instant survives a different calendar.
        XCTAssertEqual(timed.date(calendar: .current), DateCoding.parse("2026-09-04T15:30:00-07:00"))

        // What EventKit hands back.
        XCTAssertEqual(DueDate(components: DateComponents(year: 2026, month: 9, day: 5))?.hasTime, false)
        XCTAssertEqual(DueDate(components: DateComponents(year: 2026, month: 9, day: 5, hour: 8))?.hasTime, true)
        XCTAssertNil(DueDate(components: nil))
        XCTAssertNil(DueDate(components: DateComponents()))

        XCTAssertThrowsError(try DueDate.parse("soon"))
        XCTAssertThrowsError(try DueDate.parse("2026-13-45"))
    }

    func testCalendarWindowDefaults() throws {
        let now = Fixtures.now
        let window = try CalendarWindow.parse(Args([:]), now: now)
        XCTAssertEqual(window.from, now.addingTimeInterval(-7 * 86_400))
        XCTAssertEqual(window.to, now.addingTimeInterval(30 * 86_400))

        let explicit = try CalendarWindow.parse(Args(["from": "2026-09-01T00:00:00Z", "to": "2026-09-02T00:00:00Z"]), now: now)
        XCTAssertEqual(explicit, CalendarWindow(from: DateCoding.parse("2026-09-01T00:00:00Z")!, to: DateCoding.parse("2026-09-02T00:00:00Z")!))

        let fromOnly = try CalendarWindow.parse(Args(["from": "2026-09-01T00:00:00Z"]), now: now)
        XCTAssertEqual(fromOnly.to, now.addingTimeInterval(30 * 86_400))

        XCTAssertThrowsError(try CalendarWindow.parse(Args(["from": "2026-09-02T00:00:00Z", "to": "2026-09-01T00:00:00Z"]), now: now))
        XCTAssertThrowsError(try CalendarWindow.parse(Args(["from": "yesterday"]), now: now))
    }

    func testSinceFilter() {
        let since = Fixtures.date("2026-09-01T00:00:00-07:00")
        let kept = SinceFilter.apply(Fixtures.events, since: since, modifiedAt: \.modifiedAt)
        XCTAssertEqual(kept.map(\.id), ["ev-standup", "ev-far"])  // ev-old too early, ev-bday has no date
        XCTAssertEqual(SinceFilter.apply(Fixtures.events, since: nil, modifiedAt: \.modifiedAt).count, 4)
        // Boundary is inclusive.
        let exact = SinceFilter.apply(Fixtures.events, since: Fixtures.date("2026-09-01T08:00:00-07:00"), modifiedAt: \.modifiedAt)
        XCTAssertEqual(exact.map(\.id), ["ev-standup", "ev-far"])
    }

    func testTitleResolver() throws {
        let rows = Fixtures.calendars + [CalendarRow(id: "cal-home-2", title: "home", type: "local", allowsModifications: true, color: nil)]
        XCTAssertEqual(try TitleResolver.resolve("cal-home-2", in: rows, kind: "calendar", id: \.id, title: \.title).id, "cal-home-2")
        XCTAssertEqual(try TitleResolver.resolve("birthdays", in: rows, kind: "calendar", id: \.id, title: \.title).id, "cal-birthdays")
        XCTAssertThrowsError(try TitleResolver.resolve("HOME", in: rows, kind: "calendar", id: \.id, title: \.title)) { error in
            XCTAssertEqual((error as? BridgeError)?.code, "invalid_args")
        }
        XCTAssertThrowsError(try TitleResolver.resolve("Work", in: rows, kind: "calendar", id: \.id, title: \.title)) { error in
            XCTAssertEqual((error as? BridgeError)?.code, "not_found")
        }
    }

    func testPatchParsing() async throws {
        let patch = try CalendarEventPatch.parse(Args([
            "title": "Lunch", "start": "2026-09-03T12:00:00-07:00", "end": "2026-09-03T13:00:00-07:00",
            "all_day": false, "alarm_minutes_before": 15, "url": "https://x.example",
        ]))
        XCTAssertEqual(patch.title, "Lunch")
        XCTAssertEqual(patch.alarmMinutesBefore, 15)
        XCTAssertEqual(patch.allDay, false)
        XCTAssertFalse(patch.isEmpty)
        XCTAssertTrue(try CalendarEventPatch.parse(Args(["id": "x"])).isEmpty)
        XCTAssertNoThrow(try patch.validateForCreate())
        await XCTAssertBridgeError(try CalendarEventPatch.parse(Args(["start": "2026-09-03T13:00:00Z", "end": "2026-09-03T12:00:00Z"])),
                                   code: "invalid_args", messageContains: "before")
        await XCTAssertBridgeError(try CalendarEventPatch.parse(Args(["alarm_minutes_before": -1])), code: "invalid_args")
        await XCTAssertBridgeError(try CalendarEventPatch.parse(Args(["title": "x", "start": "2026-09-03T12:00:00Z"])).validateForCreate(),
                                   code: "invalid_args", messageContains: "'end'")

        let reminder = try ReminderPatch.parse(Args(["title": "Milk", "due": "2026-09-05", "priority": 5]))
        XCTAssertEqual(reminder.due?.hasTime, false)
        XCTAssertEqual(reminder.priority, 5)
        await XCTAssertBridgeError(try ReminderPatch.parse(Args(["priority": 11])), code: "invalid_args", messageContains: "priority")
        await XCTAssertBridgeError(try ReminderPatch.parse(Args(["due": "whenever"])), code: "invalid_args", messageContains: "due")
        await XCTAssertBridgeError(try ReminderPatch.parse(Args([:])).validateForCreate(), code: "invalid_args", messageContains: "title")
    }

    // MARK: calendar.*

    func testCalendarCalendars() async throws {
        let rows = try await data([CalendarRow].self, "calendar.calendars")
        XCTAssertEqual(rows, Fixtures.calendars)
    }

    func testCalendarListUsesDefaultWindowAndSince() async throws {
        let before = Date()
        let all = try await data([CalendarEventRow].self, "calendar.list")
        XCTAssertEqual(all.map(\.id), ["ev-standup", "ev-bday"])  // ev-old is before −7d, ev-far after +30d
        let query = await eventKit.eventQueries.last!
        XCTAssertEqual(query.from.timeIntervalSince1970, before.addingTimeInterval(-7 * 86_400).timeIntervalSince1970, accuracy: 5)
        XCTAssertEqual(query.to.timeIntervalSince1970, before.addingTimeInterval(30 * 86_400).timeIntervalSince1970, accuracy: 5)
        XCTAssertNil(query.calendar)

        let since = try await data([CalendarEventRow].self, "calendar.list", ["since": "2026-09-01T00:00:00-07:00"])
        XCTAssertEqual(since.map(\.id), ["ev-standup"])

        let wide = try await data([CalendarEventRow].self, "calendar.list",
                                  ["from": "2026-01-01T00:00:00Z", "to": "2027-01-01T00:00:00Z", "calendar": "Home"])
        XCTAssertEqual(wide.map(\.id), ["ev-old", "ev-standup", "ev-far"])
        let wideQuery = await eventKit.eventQueries.last
        XCTAssertEqual(wideQuery?.calendar, "Home")

        await expectError("calendar.list", ["since": "lately"], code: "invalid_args", messageContains: "RFC 3339")
        await expectError("calendar.list", ["from": "2026-09-02T00:00:00Z", "to": "2026-09-01T00:00:00Z"], code: "invalid_args")
    }

    func testCalendarGet() async throws {
        let row = try await data(CalendarEventRow.self, "calendar.get", ["id": "ev-standup"])
        XCTAssertEqual(row, Fixtures.events[1])
        await expectError("calendar.get", code: "invalid_args", messageContains: "'id'")
        await expectError("calendar.get", ["id": "nope"], code: "not_found", messageContains: "nope")
    }

    func testCalendarCreateUpdateDelete() async throws {
        let created = try await data(CalendarEventRow.self, "calendar.create", [
            "title": "Lunch", "start": "2026-09-03T12:00:00-07:00", "end": "2026-09-03T13:00:00-07:00",
            "calendar": "Home", "location": "Cafe", "alarm_minutes_before": 10,
        ])
        XCTAssertEqual(created.title, "Lunch")
        XCTAssertEqual(created.calendar, "Home")
        XCTAssertEqual(created.location, "Cafe")
        XCTAssertEqual(created.alarmCount, 1)
        XCTAssertEqual(created.start, Fixtures.date("2026-09-03T12:00:00-07:00"))
        await expectError("calendar.create", ["start": "2026-09-03T12:00:00Z", "end": "2026-09-03T13:00:00Z"],
                          code: "invalid_args", messageContains: "title")
        await expectError("calendar.create", ["title": "x", "start": "2026-09-03T12:00:00Z"], code: "invalid_args", messageContains: "'end'")

        let id = JSONValue.string(created.id)
        let updated = try await data(CalendarEventRow.self, "calendar.update", ["id": id, "title": "Long lunch"])
        XCTAssertEqual(updated.title, "Long lunch")
        XCTAssertEqual(updated.location, "Cafe")
        await expectError("calendar.update", ["id": id], code: "invalid_args", messageContains: "nothing to update")
        await expectError("calendar.update", ["title": "x"], code: "invalid_args", messageContains: "'id'")
        await expectError("calendar.update", ["id": "nope", "title": "x"], code: "not_found")

        let deleted = await call("calendar.delete", ["id": id, "future": true])
        XCTAssertEqual(deleted, Response.success(id: 1, data: ["deleted": true]))
        let deletion = await eventKit.deletedEvents.last
        XCTAssertEqual(deletion?.future, true)
        await expectError("calendar.delete", ["id": id], code: "not_found")
    }

    func testPermissionDeniedPassesThrough() async {
        await eventKit.setPermissionDenied(true)
        await expectError("calendar.calendars", code: "permission_denied", messageContains: "cider-bridge")
        await expectError("reminders.list", code: "permission_denied")
    }

    // MARK: reminders.*

    func testReminderLists() async throws {
        let lists = try await data([ReminderListRow].self, "reminders.lists")
        XCTAssertEqual(lists, Fixtures.lists)
    }

    func testRemindersListHidesCompletedByDefault() async throws {
        let open = try await data([ReminderRow].self, "reminders.list")
        XCTAssertEqual(open.map(\.id), ["rem-milk", "rem-call"])
        let openQuery = await eventKit.reminderQueries.last
        XCTAssertEqual(openQuery?.includeCompleted, false)

        let all = try await data([ReminderRow].self, "reminders.list", ["include_completed": true])
        XCTAssertEqual(all.count, 3)

        let todo = try await data([ReminderRow].self, "reminders.list", ["list": "Todo", "include_completed": "yes"])
        XCTAssertEqual(todo.map(\.id), ["rem-done", "rem-call"])

        let since = try await data([ReminderRow].self, "reminders.list", ["since": "2026-09-02T00:00:00-07:00"])
        XCTAssertEqual(since.map(\.id), ["rem-call"])
    }

    func testReminderCreateUpdateCompleteDelete() async throws {
        let created = try await data(ReminderRow.self, "reminders.create", ["title": "Eggs", "list": "Shopping", "due": "2026-09-06", "priority": 9])
        XCTAssertEqual(created.title, "Eggs")
        XCTAssertEqual(created.list, "Shopping")
        XCTAssertTrue(created.dueAllDay)
        XCTAssertEqual(created.priority, 9)
        XCTAssertFalse(created.completed)
        await expectError("reminders.create", ["list": "Shopping"], code: "invalid_args", messageContains: "title")

        let id = JSONValue.string(created.id)
        let updated = try await data(ReminderRow.self, "reminders.update", ["id": id, "due": "2026-09-06T09:00:00-07:00", "notes": "dozen"])
        XCTAssertFalse(updated.dueAllDay)
        XCTAssertEqual(updated.due, Fixtures.date("2026-09-06T09:00:00-07:00"))
        XCTAssertEqual(updated.notes, "dozen")
        await expectError("reminders.update", ["id": id], code: "invalid_args", messageContains: "nothing to update")

        let completed = try await data(ReminderRow.self, "reminders.complete", ["id": id])
        XCTAssertTrue(completed.completed)
        XCTAssertNotNil(completed.completionDate)
        let reopened = try await data(ReminderRow.self, "reminders.reopen", ["id": id])
        XCTAssertFalse(reopened.completed)
        XCTAssertNil(reopened.completionDate)
        await expectError("reminders.complete", ["id": "nope"], code: "not_found")

        let deleted = await call("reminders.delete", ["id": id])
        XCTAssertEqual(deleted, Response.success(id: 1, data: ["deleted": true]))
        let deletions = await eventKit.deletedReminders
        XCTAssertEqual(deletions, [created.id])
        await expectError("reminders.delete", code: "invalid_args", messageContains: "'id'")
    }

    // MARK: contacts.*

    func testContactsListAndGet() async throws {
        let all = try await data([ContactRow].self, "contacts.list")
        XCTAssertEqual(all.count, 3)
        let paul = try await data([ContactRow].self, "contacts.list", ["search": "Paul", "limit": 1])
        XCTAssertEqual(paul.map(\.id), ["c-paul"])
        let query = await contacts.queries.last
        XCTAssertEqual(query?.limit, 1)
        XCTAssertEqual(query?.search, "Paul")

        await expectError("contacts.list", ["since": "2026-01-01T00:00:00Z"], code: "invalid_args", messageContains: "modification date")
        await expectError("contacts.list", ["limit": 0], code: "invalid_args", messageContains: "limit")
        await expectError("contacts.list", ["limit": "lots"], code: "invalid_args", messageContains: "integer")

        let row = try await data(ContactRow.self, "contacts.get", ["id": "c-ada"])
        XCTAssertEqual(row.givenName, "Ada")
        await expectError("contacts.get", ["id": "nope"], code: "not_found")
        await expectError("contacts.get", code: "invalid_args", messageContains: "'id'")
    }

    func testCommandsAreRegistered() async {
        let commands = await router.commands
        for cmd in ["calendar.calendars", "calendar.list", "calendar.get", "calendar.create", "calendar.update", "calendar.delete",
                    "reminders.lists", "reminders.list", "reminders.create", "reminders.update", "reminders.complete",
                    "reminders.reopen", "reminders.delete", "contacts.list", "contacts.get"] {
            XCTAssertTrue(commands.contains(cmd), "missing \(cmd)")
        }
    }
}
