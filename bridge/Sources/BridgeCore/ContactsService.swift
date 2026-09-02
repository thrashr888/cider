import Foundation

// MARK: - Rows

public struct LabeledValue: Codable, Equatable, Sendable {
    /// Localized label (`home`, `work`, `mobile`, …) or `null` when unlabeled.
    public var label: String?
    public var value: String

    public init(label: String?, value: String) {
        self.label = label
        self.value = value
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(label, forKey: .label)
        try c.encode(value, forKey: .value)
    }

    private enum CodingKeys: String, CodingKey { case label, value }
}

/// A contact. `modified_at` is always `null`: `CNContact` exposes no
/// modification date in the public API, so `contacts.list` cannot honour
/// `since` (it returns `invalid_args`).
public struct ContactRow: Codable, Equatable, Sendable {
    public var id: String
    public var givenName: String?
    public var familyName: String?
    public var organization: String?
    public var nickname: String?
    public var emails: [LabeledValue]
    public var phones: [LabeledValue]
    /// `yyyy-MM-dd`, or `--MM-dd` when the year is unknown.
    public var birthday: String?
    public var modifiedAt: Date?

    public init(id: String, givenName: String?, familyName: String?, organization: String? = nil,
                nickname: String? = nil, emails: [LabeledValue] = [], phones: [LabeledValue] = [],
                birthday: String? = nil, modifiedAt: Date? = nil) {
        self.id = id
        self.givenName = givenName
        self.familyName = familyName
        self.organization = organization
        self.nickname = nickname
        self.emails = emails
        self.phones = phones
        self.birthday = birthday
        self.modifiedAt = modifiedAt
    }

    private enum CodingKeys: String, CodingKey {
        case id, organization, nickname, emails, phones, birthday
        case givenName = "given_name"
        case familyName = "family_name"
        case modifiedAt = "modified_at"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        givenName = try c.decodeIfPresent(String.self, forKey: .givenName)
        familyName = try c.decodeIfPresent(String.self, forKey: .familyName)
        organization = try c.decodeIfPresent(String.self, forKey: .organization)
        nickname = try c.decodeIfPresent(String.self, forKey: .nickname)
        emails = try c.decodeIfPresent([LabeledValue].self, forKey: .emails) ?? []
        phones = try c.decodeIfPresent([LabeledValue].self, forKey: .phones) ?? []
        birthday = try c.decodeIfPresent(String.self, forKey: .birthday)
        modifiedAt = try c.decodeDate(forKey: .modifiedAt)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(givenName, forKey: .givenName)
        try c.encode(familyName, forKey: .familyName)
        try c.encode(organization, forKey: .organization)
        try c.encode(nickname, forKey: .nickname)
        try c.encode(emails, forKey: .emails)
        try c.encode(phones, forKey: .phones)
        try c.encode(birthday, forKey: .birthday)
        try c.encodeDate(modifiedAt, forKey: .modifiedAt)
    }

    /// Contacts birthdays may omit the year.
    public static func birthdayString(year: Int?, month: Int?, day: Int?) -> String? {
        guard let month, let day else { return nil }
        if let year { return String(format: "%04d-%02d-%02d", year, month, day) }
        return String(format: "--%02d-%02d", month, day)
    }

    /// Empty strings from Contacts become `null`.
    public static func nonEmpty(_ s: String) -> String? { s.isEmpty ? nil : s }
}

// MARK: - Service protocol

public protocol ContactsService: Sendable {
    /// `search` matches names (or an address when it contains `@`); nil lists everything.
    func contacts(search: String?, limit: Int?) async throws -> [ContactRow]
    func contact(id: String) async throws -> ContactRow
}

// MARK: - Command registration

public func registerContactsCommands(_ router: CommandRouter, service: some ContactsService) async {
    await router.register("contacts.list") { raw in
        let args = Args(raw)
        if args.value("since") != nil {
            throw BridgeError.invalidArgs(
                "'since' is not supported for contacts: CNContact exposes no modification date, so modified_at is always null")
        }
        if let limit = try args.int("limit"), limit < 1 {
            throw BridgeError.invalidArgs("'limit' must be >= 1")
        }
        let rows = try await service.contacts(search: try args.string("search"), limit: try args.int("limit"))
        return try JSONValue(encoding: rows)
    }

    await router.register("contacts.get") { raw in
        try JSONValue(encoding: try await service.contact(id: try Args(raw).requiredString("id")))
    }
}

// MARK: - The real service

#if canImport(Contacts)
import Contacts

/// `ContactsService` on `CNContactStore`. Access is requested on first use;
/// a denial is `permission_denied` naming the Privacy pane and this binary.
@MainActor
public final class CNContactsService: ContactsService {
    public let store: CNContactStore

    static let keys: [CNKeyDescriptor] = [
        CNContactIdentifierKey, CNContactGivenNameKey, CNContactFamilyNameKey, CNContactOrganizationNameKey,
        CNContactNicknameKey, CNContactEmailAddressesKey, CNContactPhoneNumbersKey, CNContactBirthdayKey,
    ] as [CNKeyDescriptor]

    public init(store: CNContactStore = CNContactStore()) {
        self.store = store
    }

    /// `authorized`, `limited`, `denied`, `restricted`, `not_determined`.
    public nonisolated static var authorizationName: String {
        switch CNContactStore.authorizationStatus(for: .contacts) {
        case .authorized: return "authorized"
        case .denied: return "denied"
        case .restricted: return "restricted"
        case .notDetermined: return "not_determined"
        #if os(iOS)
        case .limited: return "limited"
        #endif
        @unknown default: return "unknown"
        }
    }

    public func authorize() async throws {
        switch CNContactStore.authorizationStatus(for: .contacts) {
        case .notDetermined:
            let granted: Bool
            do {
                granted = try await store.requestAccess(for: .contacts)
            } catch {
                throw BridgeError.permissionDenied(
                    "Contacts access request failed (\(error.localizedDescription)); "
                        + PermissionHelp.deniedMessage(service: "Contacts", pane: "Contacts"))
            }
            guard granted else {
                throw BridgeError.permissionDenied(PermissionHelp.deniedMessage(service: "Contacts", pane: "Contacts"))
            }
        case .denied, .restricted:
            throw BridgeError.permissionDenied(PermissionHelp.deniedMessage(service: "Contacts", pane: "Contacts"))
        default:
            // `.authorized`, and `.limited` on newer systems: both can read.
            return
        }
    }

    public func contacts(search: String?, limit: Int?) async throws -> [ContactRow] {
        try await authorize()
        var rows: [ContactRow] = []
        if let search, !search.trimmingCharacters(in: .whitespaces).isEmpty {
            let q = search.trimmingCharacters(in: .whitespaces)
            let predicate = q.contains("@")
                ? CNContact.predicateForContacts(matchingEmailAddress: q)
                : CNContact.predicateForContacts(matchingName: q)
            rows = try mapping { try store.unifiedContacts(matching: predicate, keysToFetch: Self.keys) }.map(\.row)
            rows.sort { $0.sortKey < $1.sortKey }
        } else {
            let request = CNContactFetchRequest(keysToFetch: Self.keys)
            request.sortOrder = .userDefault
            request.unifyResults = true
            try mapping {
                try store.enumerateContacts(with: request) { contact, stop in
                    rows.append(contact.row)
                    if let limit, rows.count >= limit { stop.pointee = true }
                }
            }
        }
        if let limit { rows = Array(rows.prefix(limit)) }
        return rows
    }

    public func contact(id: String) async throws -> ContactRow {
        try await authorize()
        do {
            return try store.unifiedContact(withIdentifier: id, keysToFetch: Self.keys).row
        } catch let error as NSError where error.domain == CNErrorDomain && error.code == CNError.recordDoesNotExist.rawValue {
            throw BridgeError.notFound("contact '\(id)' not found")
        } catch {
            throw Self.bridgeError(from: error)
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
        let message = "Contacts: \(ns.localizedDescription)"
        guard ns.domain == CNErrorDomain, let code = CNError.Code(rawValue: ns.code) else {
            return .internalError(message)
        }
        switch code {
        case .authorizationDenied:
            return .permissionDenied(PermissionHelp.deniedMessage(service: "Contacts", pane: "Contacts"))
        case .recordDoesNotExist:
            return .notFound(message)
        case .validationMultipleErrors, .validationTypeMismatch, .validationConfigurationError, .predicateInvalid,
             .insertedRecordAlreadyExists, .unauthorizedKeys:
            return .invalidArgs(message)
        default:
            return .internalError(message)
        }
    }
}

extension ContactRow {
    fileprivate var sortKey: String {
        "\(familyName ?? "") \(givenName ?? "") \(organization ?? "")".lowercased()
    }
}

extension CNContact {
    var row: ContactRow {
        ContactRow(
            id: identifier, givenName: ContactRow.nonEmpty(givenName), familyName: ContactRow.nonEmpty(familyName),
            organization: ContactRow.nonEmpty(organizationName), nickname: ContactRow.nonEmpty(nickname),
            emails: emailAddresses.map { LabeledValue(label: Self.label($0.label), value: $0.value as String) },
            phones: phoneNumbers.map { LabeledValue(label: Self.label($0.label), value: $0.value.stringValue) },
            birthday: ContactRow.birthdayString(year: birthday?.year, month: birthday?.month, day: birthday?.day),
            modifiedAt: nil)
    }

    private static func label(_ raw: String?) -> String? {
        guard let raw, !raw.isEmpty else { return nil }
        return CNLabeledValue<NSString>.localizedString(forLabel: raw).lowercased()
    }
}
#endif
