import Foundation

// MARK: - Rows (the RFC's `calendar.*` / `reminders.*` data shapes)
//
// Every row is a plain struct: the EventKit objects are mapped into these on
// the main actor (see `EKEventKitService`), and the wire encoding below is
// what the tests pin down without touching a store. Absent values encode as
// `null`, never as a missing key; dates are RFC 3339 with the local offset.

public struct CalendarRow: Codable, Equatable, Sendable {
    /// `EKCalendar.calendarIdentifier`.
    public var id: String
    public var title: String
    /// `local`, `caldav`, `exchange`, `subscription`, `birthday`.
    public var type: String
    public var allowsModifications: Bool
    /// `#RRGGBB`, when the calendar has a colour.
    public var color: String?

    public init(id: String, title: String, type: String, allowsModifications: Bool, color: String?) {
        self.id = id
        self.title = title
        self.type = type
        self.allowsModifications = allowsModifications
        self.color = color
    }

    private enum CodingKeys: String, CodingKey {
        case id, title, type, color
        case allowsModifications = "allows_modifications"
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(type, forKey: .type)
        try c.encode(allowsModifications, forKey: .allowsModifications)
        try c.encode(color, forKey: .color)
    }

    /// `#RRGGBB` from RGB(A) components in 0...1; `nil` for anything else.
    public static func hexColor(components: [Double]?) -> String? {
        guard let components, components.count >= 3 else { return nil }
        let bytes = components.prefix(3).map { Int((min(max($0, 0), 1) * 255).rounded()) }
        return String(format: "#%02X%02X%02X", bytes[0], bytes[1], bytes[2])
    }
}

public struct CalendarEventRow: Codable, Equatable, Sendable {
    /// `EKEvent.calendarItemIdentifier` (shared by every occurrence of a recurring event).
    public var id: String
    public var title: String?
    public var calendar: String?
    public var calendarID: String?
    public var location: String?
    public var start: Date?
    public var end: Date?
    public var allDay: Bool
    public var notes: String?
    public var url: String?
    /// `EKEvent.lastModifiedDate`.
    public var modifiedAt: Date?
    public var createdAt: Date?
    public var hasRecurrence: Bool
    public var attendeeCount: Int
    public var alarmCount: Int

    public init(id: String, title: String?, calendar: String?, calendarID: String?, location: String? = nil,
                start: Date?, end: Date?, allDay: Bool = false, notes: String? = nil, url: String? = nil,
                modifiedAt: Date? = nil, createdAt: Date? = nil, hasRecurrence: Bool = false,
                attendeeCount: Int = 0, alarmCount: Int = 0) {
        self.id = id
        self.title = title
        self.calendar = calendar
        self.calendarID = calendarID
        self.location = location
        self.start = start
        self.end = end
        self.allDay = allDay
        self.notes = notes
        self.url = url
        self.modifiedAt = modifiedAt
        self.createdAt = createdAt
        self.hasRecurrence = hasRecurrence
        self.attendeeCount = attendeeCount
        self.alarmCount = alarmCount
    }

    private enum CodingKeys: String, CodingKey {
        case id, title, calendar, location, start, end, notes, url
        case calendarID = "calendar_id"
        case allDay = "all_day"
        case modifiedAt = "modified_at"
        case createdAt = "created_at"
        case hasRecurrence = "has_recurrence"
        case attendeeCount = "attendee_count"
        case alarmCount = "alarm_count"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        title = try c.decodeIfPresent(String.self, forKey: .title)
        calendar = try c.decodeIfPresent(String.self, forKey: .calendar)
        calendarID = try c.decodeIfPresent(String.self, forKey: .calendarID)
        location = try c.decodeIfPresent(String.self, forKey: .location)
        start = try c.decodeDate(forKey: .start)
        end = try c.decodeDate(forKey: .end)
        allDay = try c.decodeIfPresent(Bool.self, forKey: .allDay) ?? false
        notes = try c.decodeIfPresent(String.self, forKey: .notes)
        url = try c.decodeIfPresent(String.self, forKey: .url)
        modifiedAt = try c.decodeDate(forKey: .modifiedAt)
        createdAt = try c.decodeDate(forKey: .createdAt)
        hasRecurrence = try c.decodeIfPresent(Bool.self, forKey: .hasRecurrence) ?? false
        attendeeCount = try c.decodeIfPresent(Int.self, forKey: .attendeeCount) ?? 0
        alarmCount = try c.decodeIfPresent(Int.self, forKey: .alarmCount) ?? 0
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(calendar, forKey: .calendar)
        try c.encode(calendarID, forKey: .calendarID)
        try c.encode(location, forKey: .location)
        try c.encodeDate(start, forKey: .start)
        try c.encodeDate(end, forKey: .end)
        try c.encode(allDay, forKey: .allDay)
        try c.encode(notes, forKey: .notes)
        try c.encode(url, forKey: .url)
        try c.encodeDate(modifiedAt, forKey: .modifiedAt)
        try c.encodeDate(createdAt, forKey: .createdAt)
        try c.encode(hasRecurrence, forKey: .hasRecurrence)
        try c.encode(attendeeCount, forKey: .attendeeCount)
        try c.encode(alarmCount, forKey: .alarmCount)
    }
}

public struct ReminderListRow: Codable, Equatable, Sendable {
    public var id: String
    public var title: String
    public var allowsModifications: Bool
    public var color: String?

    public init(id: String, title: String, allowsModifications: Bool, color: String?) {
        self.id = id
        self.title = title
        self.allowsModifications = allowsModifications
        self.color = color
    }

    private enum CodingKeys: String, CodingKey {
        case id, title, color
        case allowsModifications = "allows_modifications"
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(allowsModifications, forKey: .allowsModifications)
        try c.encode(color, forKey: .color)
    }
}

/// A reminder due date: EventKit stores `DateComponents`, which may carry a
/// time or just a day. `due` on the wire is RFC 3339 either way (local
/// midnight for day-only), with `due_all_day` saying which it was.
public struct DueDate: Equatable, Sendable {
    public var components: DateComponents
    public var hasTime: Bool

    public init(components: DateComponents, hasTime: Bool) {
        self.components = components
        self.hasTime = hasTime
    }

    public init(date: Date, hasTime: Bool = true, calendar: Calendar = .current) {
        let fields: Set<Calendar.Component> = hasTime ? [.year, .month, .day, .hour, .minute, .second] : [.year, .month, .day]
        var c = calendar.dateComponents(fields, from: date)
        c.timeZone = hasTime ? calendar.timeZone : nil
        self.init(components: c, hasTime: hasTime)
    }

    /// Recognizes what EventKit hands back: components with an hour are timed.
    public init?(components: DateComponents?) {
        guard let components, components.year != nil || components.day != nil else { return nil }
        self.init(components: components, hasTime: components.hour != nil)
    }

    /// `yyyy-MM-dd` is a day-only due; anything else must parse as RFC 3339.
    public static func parse(_ string: String, calendar: Calendar = .current) throws -> DueDate {
        let trimmed = string.trimmingCharacters(in: .whitespaces)
        if trimmed.count == 10, trimmed.wholeMatch(of: /\d{4}-\d{2}-\d{2}/) != nil {
            let f = DateFormatter()
            f.locale = Locale(identifier: "en_US_POSIX")
            f.timeZone = calendar.timeZone
            f.dateFormat = "yyyy-MM-dd"
            guard let date = f.date(from: trimmed) else {
                throw BridgeError.invalidArgs("'due' is not a valid date: '\(string)'")
            }
            return DueDate(date: date, hasTime: false, calendar: calendar)
        }
        guard let date = DateCoding.parse(trimmed, timeZone: calendar.timeZone) else {
            throw BridgeError.invalidArgs("'due' must be an RFC 3339 date or yyyy-MM-dd, got '\(string)'")
        }
        return DueDate(date: date, hasTime: true, calendar: calendar)
    }

    public func date(calendar: Calendar = .current) -> Date? {
        var cal = calendar
        if let tz = components.timeZone { cal.timeZone = tz }
        return cal.date(from: components)
    }
}

public struct ReminderRow: Codable, Equatable, Sendable {
    public var id: String
    public var title: String?
    public var list: String?
    public var listID: String?
    public var notes: String?
    public var completed: Bool
    public var completionDate: Date?
    public var due: Date?
    /// True when the due date is a day without a time.
    public var dueAllDay: Bool
    /// EventKit priority: 0 none, 1 high, 5 medium, 9 low.
    public var priority: Int
    public var modifiedAt: Date?
    public var createdAt: Date?
    public var url: String?

    public init(id: String, title: String?, list: String?, listID: String?, notes: String? = nil,
                completed: Bool = false, completionDate: Date? = nil, due: Date? = nil, dueAllDay: Bool = false,
                priority: Int = 0, modifiedAt: Date? = nil, createdAt: Date? = nil, url: String? = nil) {
        self.id = id
        self.title = title
        self.list = list
        self.listID = listID
        self.notes = notes
        self.completed = completed
        self.completionDate = completionDate
        self.due = due
        self.dueAllDay = dueAllDay
        self.priority = priority
        self.modifiedAt = modifiedAt
        self.createdAt = createdAt
        self.url = url
    }

    private enum CodingKeys: String, CodingKey {
        case id, title, list, notes, completed, due, priority, url
        case listID = "list_id"
        case completionDate = "completion_date"
        case dueAllDay = "due_all_day"
        case modifiedAt = "modified_at"
        case createdAt = "created_at"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        title = try c.decodeIfPresent(String.self, forKey: .title)
        list = try c.decodeIfPresent(String.self, forKey: .list)
        listID = try c.decodeIfPresent(String.self, forKey: .listID)
        notes = try c.decodeIfPresent(String.self, forKey: .notes)
        completed = try c.decodeIfPresent(Bool.self, forKey: .completed) ?? false
        completionDate = try c.decodeDate(forKey: .completionDate)
        due = try c.decodeDate(forKey: .due)
        dueAllDay = try c.decodeIfPresent(Bool.self, forKey: .dueAllDay) ?? false
        priority = try c.decodeIfPresent(Int.self, forKey: .priority) ?? 0
        modifiedAt = try c.decodeDate(forKey: .modifiedAt)
        createdAt = try c.decodeDate(forKey: .createdAt)
        url = try c.decodeIfPresent(String.self, forKey: .url)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(list, forKey: .list)
        try c.encode(listID, forKey: .listID)
        try c.encode(notes, forKey: .notes)
        try c.encode(completed, forKey: .completed)
        try c.encodeDate(completionDate, forKey: .completionDate)
        try c.encodeDate(due, forKey: .due)
        try c.encode(dueAllDay, forKey: .dueAllDay)
        try c.encode(priority, forKey: .priority)
        try c.encodeDate(modifiedAt, forKey: .modifiedAt)
        try c.encodeDate(createdAt, forKey: .createdAt)
        try c.encode(url, forKey: .url)
    }
}

extension KeyedEncodingContainer {
    /// RFC 3339 string, or `null` when the date is absent.
    mutating func encodeDate(_ date: Date?, forKey key: Key) throws {
        try encode(date.map { DateCoding.format($0) }, forKey: key)
    }
}

extension KeyedDecodingContainer {
    func decodeDate(forKey key: Key) throws -> Date? {
        guard let s = try decodeIfPresent(String.self, forKey: key) else { return nil }
        guard let d = DateCoding.parse(s) else {
            throw DecodingError.dataCorruptedError(forKey: key, in: self, debugDescription: "not an RFC 3339 date: \(s)")
        }
        return d
    }
}

// MARK: - Patches (create/update arguments)

/// Fields for `calendar.create` / `calendar.update`; `nil` leaves a field alone.
public struct CalendarEventPatch: Equatable, Sendable {
    public var title: String?
    public var start: Date?
    public var end: Date?
    public var calendar: String?
    public var location: String?
    public var notes: String?
    public var url: String?
    public var allDay: Bool?
    public var alarmMinutesBefore: Int?

    public init(title: String? = nil, start: Date? = nil, end: Date? = nil, calendar: String? = nil,
                location: String? = nil, notes: String? = nil, url: String? = nil, allDay: Bool? = nil,
                alarmMinutesBefore: Int? = nil) {
        self.title = title
        self.start = start
        self.end = end
        self.calendar = calendar
        self.location = location
        self.notes = notes
        self.url = url
        self.allDay = allDay
        self.alarmMinutesBefore = alarmMinutesBefore
    }

    public var isEmpty: Bool { self == CalendarEventPatch() }

    public static func parse(_ args: Args) throws -> CalendarEventPatch {
        let patch = CalendarEventPatch(
            title: try args.string("title"), start: try args.date("start"), end: try args.date("end"),
            calendar: try args.string("calendar"), location: try args.string("location"),
            notes: try args.string("notes"), url: try args.string("url"), allDay: try args.bool("all_day"),
            alarmMinutesBefore: try args.int("alarm_minutes_before"))
        if let start = patch.start, let end = patch.end, end < start {
            throw BridgeError.invalidArgs("'end' must not be before 'start'")
        }
        if let minutes = patch.alarmMinutesBefore, minutes < 0 {
            throw BridgeError.invalidArgs("'alarm_minutes_before' must be >= 0")
        }
        return patch
    }

    /// `calendar.create` needs a title and both dates.
    public func validateForCreate() throws {
        guard let title, !title.trimmingCharacters(in: .whitespaces).isEmpty else {
            throw BridgeError.invalidArgs("'title' is required")
        }
        guard start != nil else { throw BridgeError.invalidArgs("'start' is required") }
        guard end != nil else { throw BridgeError.invalidArgs("'end' is required") }
    }
}

/// Fields for `reminders.create` / `reminders.update`.
public struct ReminderPatch: Equatable, Sendable {
    public var title: String?
    public var list: String?
    public var due: DueDate?
    public var notes: String?
    public var priority: Int?
    public var url: String?

    public init(title: String? = nil, list: String? = nil, due: DueDate? = nil, notes: String? = nil,
                priority: Int? = nil, url: String? = nil) {
        self.title = title
        self.list = list
        self.due = due
        self.notes = notes
        self.priority = priority
        self.url = url
    }

    public var isEmpty: Bool { self == ReminderPatch() }

    public static func parse(_ args: Args) throws -> ReminderPatch {
        let patch = ReminderPatch(
            title: try args.string("title"), list: try args.string("list"),
            due: try args.string("due").map { try DueDate.parse($0) }, notes: try args.string("notes"),
            priority: try args.int("priority"), url: try args.string("url"))
        if let p = patch.priority, !(0...9).contains(p) {
            throw BridgeError.invalidArgs("'priority' must be 0 (none), 1 (high), 5 (medium), or 9 (low)")
        }
        return patch
    }

    public func validateForCreate() throws {
        guard let title, !title.trimmingCharacters(in: .whitespaces).isEmpty else {
            throw BridgeError.invalidArgs("'title' is required")
        }
    }
}

// MARK: - Service protocol

/// The `calendar.*` / `reminders.*` surface. Calendars and lists are addressed
/// by identifier or case-insensitive title; a nil calendar/list means the
/// store's default. Items are addressed by `calendarItemIdentifier`.
public protocol EventKitService: Sendable {
    func calendars() async throws -> [CalendarRow]
    /// Occurrences overlapping `from..<to`, optionally in one calendar.
    func events(from: Date, to: Date, calendar: String?) async throws -> [CalendarEventRow]
    func event(id: String) async throws -> CalendarEventRow
    func createEvent(_ patch: CalendarEventPatch) async throws -> CalendarEventRow
    /// `future` applies the change to this and all later occurrences.
    func updateEvent(id: String, _ patch: CalendarEventPatch, future: Bool) async throws -> CalendarEventRow
    func deleteEvent(id: String, future: Bool) async throws

    func reminderLists() async throws -> [ReminderListRow]
    func reminders(list: String?, includeCompleted: Bool) async throws -> [ReminderRow]
    func createReminder(_ patch: ReminderPatch) async throws -> ReminderRow
    func updateReminder(id: String, _ patch: ReminderPatch) async throws -> ReminderRow
    func setReminderCompleted(id: String, _ completed: Bool) async throws -> ReminderRow
    func deleteReminder(id: String) async throws
}

// MARK: - Shared query helpers

/// `calendar.list` window: `from` defaults to now − 7 days, `to` to now + 30
/// days (cider's own defaults).
public struct CalendarWindow: Equatable, Sendable {
    public static let defaultDaysBack: TimeInterval = 7
    public static let defaultDaysAhead: TimeInterval = 30

    public var from: Date
    public var to: Date

    public init(from: Date, to: Date) {
        self.from = from
        self.to = to
    }

    public static func parse(_ args: Args, now: Date = Date()) throws -> CalendarWindow {
        let from = try args.date("from") ?? now.addingTimeInterval(-defaultDaysBack * 86_400)
        let to = try args.date("to") ?? now.addingTimeInterval(defaultDaysAhead * 86_400)
        guard to > from else {
            throw BridgeError.invalidArgs("'to' (\(DateCoding.format(to))) must be after 'from' (\(DateCoding.format(from)))")
        }
        return CalendarWindow(from: from, to: to)
    }
}

public enum SinceFilter {
    /// Keeps rows whose modification date is at or after `since`. Rows with no
    /// modification date cannot prove they changed and are dropped.
    public static func apply<T>(_ rows: [T], since: Date?, modifiedAt: (T) -> Date?) -> [T] {
        guard let since else { return rows }
        return rows.filter { row in
            guard let modified = modifiedAt(row) else { return false }
            return modified >= since
        }
    }
}

/// Identifier match first, then case-insensitive title; ambiguity is `invalid_args`.
public enum TitleResolver {
    public static func resolve<T>(_ query: String, in items: [T], kind: String,
                                  id: (T) -> String, title: (T) -> String) throws -> T {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        if let hit = items.first(where: { id($0) == trimmed }) { return hit }
        let matches = items.filter { title($0).caseInsensitiveCompare(trimmed) == .orderedSame }
        switch matches.count {
        case 0:
            throw BridgeError.notFound("\(kind) '\(query)' not found")
        case 1:
            return matches[0]
        default:
            let ids = matches.map(id).joined(separator: ", ")
            throw BridgeError.invalidArgs("\(kind) '\(query)' is ambiguous (\(matches.count) matches); use an id: \(ids)")
        }
    }
}

/// Where the user goes to fix a TCC denial, and which binary to look for.
public enum PermissionHelp {
    public static var executablePath: String {
        let path = Bundle.main.executableURL?.path ?? CommandLine.arguments.first ?? "cider-bridge"
        return URL(fileURLWithPath: path).resolvingSymlinksInPath().path
    }

    /// Who TCC holds responsible: this binary when it disclaimed
    /// responsibility, otherwise the app that launched it.
    public static var subject: String {
        #if os(macOS)
        if Responsibility.isDisclaimed { return "Cider Bridge (\(executablePath))" }
        #endif
        return "the app that launched \(executablePath) (Terminal, or whatever ran cider)"
    }

    public static func deniedMessage(service: String, pane: String) -> String {
        var message = "\(service) access is denied for \(subject); allow it in System Settings › Privacy & Security › \(pane)"
        #if os(macOS)
        if !Responsibility.isDisclaimed {
            message += ", or set \(Responsibility.optIn)=1 to have cider-bridge ask for itself"
        }
        #endif
        return message
    }

    public static func writeOnlyMessage(service: String, pane: String) -> String {
        "\(service) access is add-only for \(subject); grant Full Access in System Settings › Privacy & Security › \(pane)"
    }
}

// MARK: - Command registration

/// Registers every `calendar.*` and `reminders.*` command against `service`.
/// Shared by the CLI (with `EKEventKitService`) and the tests (with a fake).
public func registerEventKitCommands(_ router: CommandRouter, service: some EventKitService) async {
    await router.register("calendar.calendars") { _ in
        try JSONValue(encoding: try await service.calendars())
    }

    await router.register("calendar.list") { raw in
        let args = Args(raw)
        let window = try CalendarWindow.parse(args)
        let since = try args.date("since")
        let rows = try await service.events(from: window.from, to: window.to, calendar: try args.string("calendar"))
        return try JSONValue(encoding: SinceFilter.apply(rows, since: since, modifiedAt: \.modifiedAt))
    }

    await router.register("calendar.get") { raw in
        try JSONValue(encoding: try await service.event(id: try Args(raw).requiredString("id")))
    }

    await router.register("calendar.create") { raw in
        let patch = try CalendarEventPatch.parse(Args(raw))
        try patch.validateForCreate()
        return try JSONValue(encoding: try await service.createEvent(patch))
    }

    await router.register("calendar.update") { raw in
        let args = Args(raw)
        let id = try args.requiredString("id")
        let patch = try CalendarEventPatch.parse(args)
        guard !patch.isEmpty else { throw BridgeError.invalidArgs("nothing to update: pass at least one field") }
        return try JSONValue(encoding: try await service.updateEvent(id: id, patch, future: try args.bool("future") ?? false))
    }

    await router.register("calendar.delete") { raw in
        let args = Args(raw)
        try await service.deleteEvent(id: try args.requiredString("id"), future: try args.bool("future") ?? false)
        return ["deleted": true]
    }

    await router.register("reminders.lists") { _ in
        try JSONValue(encoding: try await service.reminderLists())
    }

    await router.register("reminders.list") { raw in
        let args = Args(raw)
        let since = try args.date("since")
        let rows = try await service.reminders(
            list: try args.string("list"), includeCompleted: try args.bool("include_completed") ?? false)
        return try JSONValue(encoding: SinceFilter.apply(rows, since: since, modifiedAt: \.modifiedAt))
    }

    await router.register("reminders.create") { raw in
        let patch = try ReminderPatch.parse(Args(raw))
        try patch.validateForCreate()
        return try JSONValue(encoding: try await service.createReminder(patch))
    }

    await router.register("reminders.update") { raw in
        let args = Args(raw)
        let id = try args.requiredString("id")
        let patch = try ReminderPatch.parse(args)
        guard !patch.isEmpty else { throw BridgeError.invalidArgs("nothing to update: pass at least one field") }
        return try JSONValue(encoding: try await service.updateReminder(id: id, patch))
    }

    await router.register("reminders.complete") { raw in
        try JSONValue(encoding: try await service.setReminderCompleted(id: try Args(raw).requiredString("id"), true))
    }

    await router.register("reminders.reopen") { raw in
        try JSONValue(encoding: try await service.setReminderCompleted(id: try Args(raw).requiredString("id"), false))
    }

    await router.register("reminders.delete") { raw in
        try await service.deleteReminder(id: try Args(raw).requiredString("id"))
        return ["deleted": true]
    }
}

// MARK: - The real service

#if canImport(EventKit)
import EventKit

/// `EventKitService` on one `EKEventStore`. EventKit objects are not
/// `Sendable`, so the service lives on the main actor and only rows leave it.
/// Full access is requested on first use of each store; a denial is
/// `permission_denied` naming the Privacy pane and this binary.
@MainActor
public final class EKEventKitService: EventKitService {
    public let store: EKEventStore

    public init(store: EKEventStore = EKEventStore()) {
        self.store = store
    }

    // MARK: Authorization

    /// `full_access`, `write_only`, `denied`, `restricted`, `not_determined`.
    public nonisolated static func authorizationName(for entity: EKEntityType) -> String {
        switch EKEventStore.authorizationStatus(for: entity) {
        case .fullAccess: "full_access"
        case .writeOnly: "write_only"
        case .denied: "denied"
        case .restricted: "restricted"
        case .notDetermined: "not_determined"
        case .authorized: "full_access"
        @unknown default: "unknown"
        }
    }

    /// Prompts once if undetermined; throws `permission_denied` otherwise.
    public func authorize(_ entity: EKEntityType) async throws {
        let (service, pane) = entity == .event ? ("Calendar", "Calendars") : ("Reminders", "Reminders")
        switch EKEventStore.authorizationStatus(for: entity) {
        case .fullAccess, .authorized:
            return
        case .notDetermined:
            let granted: Bool
            do {
                granted = entity == .event
                    ? try await store.requestFullAccessToEvents()
                    : try await store.requestFullAccessToReminders()
            } catch {
                throw BridgeError.permissionDenied(
                    "\(service) access request failed (\(error.localizedDescription)); "
                        + PermissionHelp.deniedMessage(service: service, pane: pane))
            }
            guard granted else {
                throw BridgeError.permissionDenied(PermissionHelp.deniedMessage(service: service, pane: pane))
            }
        case .writeOnly:
            throw BridgeError.permissionDenied(PermissionHelp.writeOnlyMessage(service: service, pane: pane))
        case .denied, .restricted:
            throw BridgeError.permissionDenied(PermissionHelp.deniedMessage(service: service, pane: pane))
        @unknown default:
            throw BridgeError.permissionDenied(PermissionHelp.deniedMessage(service: service, pane: pane))
        }
    }

    // MARK: Calendars

    public func calendars() async throws -> [CalendarRow] {
        try await authorize(.event)
        return store.calendars(for: .event).map(\.calendarRow).sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    public func events(from: Date, to: Date, calendar: String?) async throws -> [CalendarEventRow] {
        try await authorize(.event)
        let calendars = try calendar.map { [try resolveCalendar($0, for: .event)] }
        let predicate = store.predicateForEvents(withStart: from, end: to, calendars: calendars)
        return store.events(matching: predicate)
            .sorted { ($0.startDate ?? .distantPast) < ($1.startDate ?? .distantPast) }
            .map(\.eventRow)
    }

    public func event(id: String) async throws -> CalendarEventRow {
        try await authorize(.event)
        return try findEvent(id).eventRow
    }

    public func createEvent(_ patch: CalendarEventPatch) async throws -> CalendarEventRow {
        try await authorize(.event)
        let event = EKEvent(eventStore: store)
        event.calendar = try patch.calendar.map { try resolveCalendar($0, for: .event) }
            ?? store.defaultCalendarForNewEvents
        try apply(patch, to: event)
        try mapping { try store.save(event, span: .thisEvent, commit: true) }
        return event.eventRow
    }

    public func updateEvent(id: String, _ patch: CalendarEventPatch, future: Bool) async throws -> CalendarEventRow {
        try await authorize(.event)
        let event = try findEvent(id)
        if let calendar = patch.calendar { event.calendar = try resolveCalendar(calendar, for: .event) }
        try apply(patch, to: event)
        try mapping { try store.save(event, span: future ? .futureEvents : .thisEvent, commit: true) }
        return event.eventRow
    }

    public func deleteEvent(id: String, future: Bool) async throws {
        try await authorize(.event)
        let event = try findEvent(id)
        try mapping { try store.remove(event, span: future ? .futureEvents : .thisEvent, commit: true) }
    }

    // MARK: Reminders

    public func reminderLists() async throws -> [ReminderListRow] {
        try await authorize(.reminder)
        return store.calendars(for: .reminder).map(\.reminderListRow)
            .sorted { $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending }
    }

    public func reminders(list: String?, includeCompleted: Bool) async throws -> [ReminderRow] {
        try await authorize(.reminder)
        let calendars = try list.map { [try resolveCalendar($0, for: .reminder)] }
        let predicate = store.predicateForReminders(in: calendars)
        let rows: [ReminderRow] = await withCheckedContinuation { continuation in
            store.fetchReminders(matching: predicate) { reminders in
                continuation.resume(returning: (reminders ?? []).map(\.reminderRow))
            }
        }
        return rows.filter { includeCompleted || !$0.completed }
    }

    public func createReminder(_ patch: ReminderPatch) async throws -> ReminderRow {
        try await authorize(.reminder)
        let reminder = EKReminder(eventStore: store)
        reminder.calendar = try patch.list.map { try resolveCalendar($0, for: .reminder) }
            ?? store.defaultCalendarForNewReminders()
        try apply(patch, to: reminder)
        try mapping { try store.save(reminder, commit: true) }
        return reminder.reminderRow
    }

    public func updateReminder(id: String, _ patch: ReminderPatch) async throws -> ReminderRow {
        try await authorize(.reminder)
        let reminder = try findReminder(id)
        if let list = patch.list { reminder.calendar = try resolveCalendar(list, for: .reminder) }
        try apply(patch, to: reminder)
        try mapping { try store.save(reminder, commit: true) }
        return reminder.reminderRow
    }

    public func setReminderCompleted(id: String, _ completed: Bool) async throws -> ReminderRow {
        try await authorize(.reminder)
        let reminder = try findReminder(id)
        reminder.isCompleted = completed
        try mapping { try store.save(reminder, commit: true) }
        return reminder.reminderRow
    }

    public func deleteReminder(id: String) async throws {
        try await authorize(.reminder)
        let reminder = try findReminder(id)
        try mapping { try store.remove(reminder, commit: true) }
    }

    // MARK: Internals

    private func resolveCalendar(_ query: String, for entity: EKEntityType) throws -> EKCalendar {
        try TitleResolver.resolve(
            query, in: store.calendars(for: entity), kind: entity == .event ? "calendar" : "list",
            id: \.calendarIdentifier, title: \.title)
    }

    /// Accepts a `calendarItemIdentifier` (the row id) or an `eventIdentifier`.
    private func findEvent(_ id: String) throws -> EKEvent {
        if let event = store.calendarItem(withIdentifier: id) as? EKEvent { return event }
        if let event = store.event(withIdentifier: id) { return event }
        throw BridgeError.notFound("event '\(id)' not found")
    }

    private func findReminder(_ id: String) throws -> EKReminder {
        guard let reminder = store.calendarItem(withIdentifier: id) as? EKReminder else {
            throw BridgeError.notFound("reminder '\(id)' not found")
        }
        return reminder
    }

    private func apply(_ patch: CalendarEventPatch, to event: EKEvent) throws {
        if let title = patch.title { event.title = title }
        if let start = patch.start { event.startDate = start }
        if let end = patch.end { event.endDate = end }
        if let allDay = patch.allDay { event.isAllDay = allDay }
        if let location = patch.location { event.location = location.isEmpty ? nil : location }
        if let notes = patch.notes { event.notes = notes.isEmpty ? nil : notes }
        if let url = patch.url {
            guard url.isEmpty || URL(string: url) != nil else { throw BridgeError.invalidArgs("'url' is not a valid URL") }
            event.url = url.isEmpty ? nil : URL(string: url)
        }
        if let minutes = patch.alarmMinutesBefore {
            event.alarms = [EKAlarm(relativeOffset: -TimeInterval(minutes) * 60)]
        }
        if let start = event.startDate, let end = event.endDate, end < start {
            throw BridgeError.invalidArgs("'end' must not be before 'start'")
        }
    }

    private func apply(_ patch: ReminderPatch, to reminder: EKReminder) throws {
        if let title = patch.title { reminder.title = title }
        if let notes = patch.notes { reminder.notes = notes.isEmpty ? nil : notes }
        if let priority = patch.priority { reminder.priority = priority }
        if let url = patch.url {
            guard url.isEmpty || URL(string: url) != nil else { throw BridgeError.invalidArgs("'url' is not a valid URL") }
            reminder.url = url.isEmpty ? nil : URL(string: url)
        }
        if let due = patch.due {
            reminder.dueDateComponents = due.components
            // Like the Reminders app: a timed due date carries the alarm that
            // makes it notify; a day-only due date does not.
            reminder.alarms = due.hasTime ? due.date().map { [EKAlarm(absoluteDate: $0)] } : nil
        }
    }

    private func mapping<T>(_ body: () throws -> T) throws -> T {
        do {
            return try body()
        } catch let error as BridgeError {
            throw error
        } catch {
            throw Self.bridgeError(from: error)
        }
    }

    nonisolated static func bridgeError(from error: Error) -> BridgeError {
        let ns = error as NSError
        let message = "EventKit: \(ns.localizedDescription)"
        guard ns.domain == EKErrorDomain, let code = EKError.Code(rawValue: ns.code) else {
            return .internalError(message)
        }
        switch code {
        case .eventStoreNotAuthorized:
            return .permissionDenied(message)
        case .eventNotMutable, .noCalendar, .noStartDate, .noEndDate, .datesInverted, .calendarReadOnly,
             .invalidSpan, .calendarDoesNotAllowEvents, .calendarDoesNotAllowReminders, .alarmGreaterThanRecurrence,
             .durationGreaterThanRecurrence, .startDateTooFarInFuture, .startDateCollidesWithOtherOccurrence,
             .invalidEntityType, .invalidInviteReplyCalendar, .invitesCannotBeMoved, .recurringReminderRequiresDueDate,
             .reminderLocationsNotSupported, .priorityIsInvalid, .objectBelongsToDifferentStore, .calendarIsImmutable,
             .sourceDoesNotAllowEvents, .sourceDoesNotAllowReminders:
            return .invalidArgs(message)
        default:
            return .internalError(message)
        }
    }
}

extension EKCalendar {
    var typeName: String {
        switch type {
        case .local: "local"
        case .calDAV: "caldav"
        case .exchange: "exchange"
        case .subscription: "subscription"
        case .birthday: "birthday"
        @unknown default: "unknown"
        }
    }

    var hexColor: String? {
        CalendarRow.hexColor(components: cgColor?.components?.map(Double.init))
    }

    var calendarRow: CalendarRow {
        CalendarRow(id: calendarIdentifier, title: title, type: typeName, allowsModifications: allowsContentModifications,
                    color: hexColor)
    }

    var reminderListRow: ReminderListRow {
        ReminderListRow(id: calendarIdentifier, title: title, allowsModifications: allowsContentModifications,
                        color: hexColor)
    }
}

extension EKEvent {
    var eventRow: CalendarEventRow {
        CalendarEventRow(
            id: calendarItemIdentifier, title: title, calendar: calendar?.title, calendarID: calendar?.calendarIdentifier,
            location: location, start: startDate, end: endDate, allDay: isAllDay, notes: notes,
            url: url?.absoluteString, modifiedAt: lastModifiedDate, createdAt: creationDate,
            hasRecurrence: hasRecurrenceRules, attendeeCount: attendees?.count ?? 0, alarmCount: alarms?.count ?? 0)
    }
}

extension EKReminder {
    var reminderRow: ReminderRow {
        let due = DueDate(components: dueDateComponents)
        return ReminderRow(
            id: calendarItemIdentifier, title: title, list: calendar?.title, listID: calendar?.calendarIdentifier,
            notes: notes, completed: isCompleted, completionDate: completionDate, due: due?.date(),
            dueAllDay: due.map { !$0.hasTime } ?? false, priority: priority, modifiedAt: lastModifiedDate,
            createdAt: creationDate, url: url?.absoluteString)
    }
}
#endif
