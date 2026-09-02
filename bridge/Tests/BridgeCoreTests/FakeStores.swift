import Foundation
@testable import BridgeCore

/// In-memory `EventKitService`: rows are the model, and every call is
/// recorded so tests can check what the command layer asked for.
actor FakeEventKitService: EventKitService {
    var calendarRows: [CalendarRow]
    var eventRows: [CalendarEventRow]
    var listRows: [ReminderListRow]
    var reminderRows: [ReminderRow]
    var permissionDenied = false

    private(set) var eventQueries: [(from: Date, to: Date, calendar: String?)] = []
    private(set) var reminderQueries: [(list: String?, includeCompleted: Bool)] = []
    private(set) var deletedEvents: [(id: String, future: Bool)] = []
    private(set) var deletedReminders: [String] = []

    init(calendars: [CalendarRow] = [], events: [CalendarEventRow] = [], lists: [ReminderListRow] = [],
         reminders: [ReminderRow] = []) {
        calendarRows = calendars
        eventRows = events
        listRows = lists
        reminderRows = reminders
    }

    func setPermissionDenied(_ value: Bool) { permissionDenied = value }

    private func check() throws {
        if permissionDenied { throw BridgeError.permissionDenied("Calendar access is denied for /x/cider-bridge") }
    }

    func calendars() async throws -> [CalendarRow] {
        try check()
        return calendarRows
    }

    func events(from: Date, to: Date, calendar: String?) async throws -> [CalendarEventRow] {
        try check()
        eventQueries.append((from, to, calendar))
        return eventRows.filter { row in
            guard let start = row.start else { return true }
            return start >= from && start < to && (calendar == nil || row.calendar == calendar)
        }
    }

    func event(id: String) async throws -> CalendarEventRow {
        try check()
        guard let row = eventRows.first(where: { $0.id == id }) else { throw BridgeError.notFound("event '\(id)' not found") }
        return row
    }

    func createEvent(_ patch: CalendarEventPatch) async throws -> CalendarEventRow {
        try check()
        let row = CalendarEventRow(
            id: "new-\(eventRows.count + 1)", title: patch.title, calendar: patch.calendar ?? "Default",
            calendarID: "cal-default", location: patch.location, start: patch.start, end: patch.end,
            allDay: patch.allDay ?? false, notes: patch.notes, url: patch.url, modifiedAt: Date(), createdAt: Date(),
            alarmCount: patch.alarmMinutesBefore == nil ? 0 : 1)
        eventRows.append(row)
        return row
    }

    func updateEvent(id: String, _ patch: CalendarEventPatch, future: Bool) async throws -> CalendarEventRow {
        try check()
        guard let i = eventRows.firstIndex(where: { $0.id == id }) else { throw BridgeError.notFound("event '\(id)' not found") }
        if let t = patch.title { eventRows[i].title = t }
        if let s = patch.start { eventRows[i].start = s }
        if let e = patch.end { eventRows[i].end = e }
        if let l = patch.location { eventRows[i].location = l }
        eventRows[i].modifiedAt = Date()
        return eventRows[i]
    }

    func deleteEvent(id: String, future: Bool) async throws {
        try check()
        guard eventRows.contains(where: { $0.id == id }) else { throw BridgeError.notFound("event '\(id)' not found") }
        eventRows.removeAll { $0.id == id }
        deletedEvents.append((id, future))
    }

    func reminderLists() async throws -> [ReminderListRow] {
        try check()
        return listRows
    }

    func reminders(list: String?, includeCompleted: Bool) async throws -> [ReminderRow] {
        try check()
        reminderQueries.append((list, includeCompleted))
        return reminderRows.filter { (list == nil || $0.list == list) && (includeCompleted || !$0.completed) }
    }

    func createReminder(_ patch: ReminderPatch) async throws -> ReminderRow {
        try check()
        let row = ReminderRow(
            id: "rem-\(reminderRows.count + 1)", title: patch.title, list: patch.list ?? "Reminders", listID: "list-default",
            notes: patch.notes, due: patch.due?.date(), dueAllDay: patch.due.map { !$0.hasTime } ?? false,
            priority: patch.priority ?? 0, modifiedAt: Date(), createdAt: Date(), url: patch.url)
        reminderRows.append(row)
        return row
    }

    func updateReminder(id: String, _ patch: ReminderPatch) async throws -> ReminderRow {
        try check()
        guard let i = reminderRows.firstIndex(where: { $0.id == id }) else { throw BridgeError.notFound("reminder '\(id)' not found") }
        if let t = patch.title { reminderRows[i].title = t }
        if let n = patch.notes { reminderRows[i].notes = n }
        if let p = patch.priority { reminderRows[i].priority = p }
        if let d = patch.due {
            reminderRows[i].due = d.date()
            reminderRows[i].dueAllDay = !d.hasTime
        }
        return reminderRows[i]
    }

    func setReminderCompleted(id: String, _ completed: Bool) async throws -> ReminderRow {
        try check()
        guard let i = reminderRows.firstIndex(where: { $0.id == id }) else { throw BridgeError.notFound("reminder '\(id)' not found") }
        reminderRows[i].completed = completed
        reminderRows[i].completionDate = completed ? Date() : nil
        return reminderRows[i]
    }

    func deleteReminder(id: String) async throws {
        try check()
        guard reminderRows.contains(where: { $0.id == id }) else { throw BridgeError.notFound("reminder '\(id)' not found") }
        reminderRows.removeAll { $0.id == id }
        deletedReminders.append(id)
    }
}

actor FakeContactsService: ContactsService {
    var rows: [ContactRow]
    private(set) var queries: [(search: String?, limit: Int?)] = []

    init(rows: [ContactRow]) { self.rows = rows }

    func contacts(search: String?, limit: Int?) async throws -> [ContactRow] {
        queries.append((search, limit))
        var hits = rows
        if let search {
            hits = hits.filter { "\($0.givenName ?? "") \($0.familyName ?? "")".localizedCaseInsensitiveContains(search) }
        }
        if let limit { hits = Array(hits.prefix(limit)) }
        return hits
    }

    func contact(id: String) async throws -> ContactRow {
        guard let row = rows.first(where: { $0.id == id }) else { throw BridgeError.notFound("contact '\(id)' not found") }
        return row
    }
}

// MARK: - Fixtures

enum Fixtures {
    static let tz = TimeZone(identifier: "America/Los_Angeles")!
    static let now = DateCoding.parse("2026-09-02T12:00:00-07:00")!

    static func date(_ s: String) -> Date { DateCoding.parse(s)! }

    static let calendars = [
        CalendarRow(id: "cal-home", title: "Home", type: "caldav", allowsModifications: true, color: "#FF0000"),
        CalendarRow(id: "cal-birthdays", title: "Birthdays", type: "birthday", allowsModifications: false, color: nil),
    ]

    static let events = [
        CalendarEventRow(
            id: "ev-old", title: "Old", calendar: "Home", calendarID: "cal-home",
            start: date("2026-08-01T09:00:00-07:00"), end: date("2026-08-01T10:00:00-07:00"),
            modifiedAt: date("2026-08-01T00:00:00-07:00")),
        CalendarEventRow(
            id: "ev-standup", title: "Standup", calendar: "Home", calendarID: "cal-home", location: "Zoom",
            start: date("2026-09-03T09:00:00-07:00"), end: date("2026-09-03T09:15:00-07:00"),
            notes: "daily", url: "https://example.com/standup", modifiedAt: date("2026-09-01T08:00:00-07:00"),
            createdAt: date("2026-01-01T00:00:00-08:00"), hasRecurrence: true, attendeeCount: 3, alarmCount: 1),
        CalendarEventRow(
            id: "ev-bday", title: "Ada's Birthday", calendar: "Birthdays", calendarID: "cal-birthdays",
            start: date("2026-09-10T00:00:00-07:00"), end: date("2026-09-11T00:00:00-07:00"), allDay: true,
            modifiedAt: nil),
        CalendarEventRow(
            id: "ev-far", title: "Far", calendar: "Home", calendarID: "cal-home",
            start: date("2026-12-01T09:00:00-08:00"), end: date("2026-12-01T10:00:00-08:00"),
            modifiedAt: date("2026-09-02T00:00:00-07:00")),
    ]

    static let lists = [
        ReminderListRow(id: "list-todo", title: "Todo", allowsModifications: true, color: "#0000FF"),
        ReminderListRow(id: "list-shop", title: "Shopping", allowsModifications: true, color: nil),
    ]

    static let reminders = [
        ReminderRow(id: "rem-milk", title: "Milk", list: "Shopping", listID: "list-shop",
                    due: date("2026-09-05T00:00:00-07:00"), dueAllDay: true, priority: 5,
                    modifiedAt: date("2026-09-01T00:00:00-07:00")),
        ReminderRow(id: "rem-done", title: "Done thing", list: "Todo", listID: "list-todo", completed: true,
                    completionDate: date("2026-08-30T10:00:00-07:00"), modifiedAt: date("2026-08-30T10:00:00-07:00")),
        ReminderRow(id: "rem-call", title: "Call dentist", list: "Todo", listID: "list-todo", notes: "ask about x",
                    due: date("2026-09-04T15:30:00-07:00"), priority: 1, modifiedAt: date("2026-09-02T01:00:00-07:00"),
                    url: "https://example.com"),
    ]

    static let contacts = [
        ContactRow(id: "c-paul", givenName: "Paul", familyName: "Thrasher", organization: "Cider",
                   emails: [LabeledValue(label: "home", value: "paul@example.com")],
                   phones: [LabeledValue(label: nil, value: "+1 555 0100")], birthday: "--05-04"),
        ContactRow(id: "c-ada", givenName: "Ada", familyName: "Lovelace"),
        ContactRow(id: "c-paula", givenName: "Paula", familyName: nil),
    ]
}
