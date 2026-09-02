import Foundation

// MARK: - Rows (the RFC's `weather.*` data shapes)
//
// Metric units throughout: °C, km/h, mb, km. `humidity` and every
// `precipitation_chance` are whole percentages (0–100). Dates are RFC 3339
// with the local offset; a forecast day's `date` is `yyyy-MM-dd` in the
// local time zone.

/// Apple's required attribution, from `WeatherKit.WeatherService.attribution`.
public struct WeatherAttribution: Codable, Equatable, Sendable {
    public var legalURL: String
    public var serviceName: String
    public var logoLightURL: String?
    public var logoDarkURL: String?

    public init(legalURL: String, serviceName: String = "Apple Weather", logoLightURL: String? = nil,
                logoDarkURL: String? = nil) {
        self.legalURL = legalURL
        self.serviceName = serviceName
        self.logoLightURL = logoLightURL
        self.logoDarkURL = logoDarkURL
    }

    private enum CodingKeys: String, CodingKey {
        case legalURL = "legal_url"
        case serviceName = "service_name"
        case logoLightURL = "logo_light_url"
        case logoDarkURL = "logo_dark_url"
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(legalURL, forKey: .legalURL)
        try c.encode(serviceName, forKey: .serviceName)
        try c.encode(logoLightURL, forKey: .logoLightURL)
        try c.encode(logoDarkURL, forKey: .logoDarkURL)
    }
}

public struct CurrentWeatherRow: Codable, Equatable, Sendable {
    public var asOf: Date
    public var temperatureC: Double
    public var apparentTemperatureC: Double
    /// WeatherKit condition name (`clear`, `partlyCloudy`, `rain`, …).
    public var condition: String
    /// SF Symbol name (`sun.max`, `cloud.rain`, …).
    public var symbol: String
    public var humidity: Int
    public var windKph: Double
    public var windDirectionDeg: Double
    public var pressureMb: Double
    public var uvIndex: Int
    public var visibilityKm: Double
    public var isDaylight: Bool
    public var attribution: WeatherAttribution

    public init(asOf: Date, temperatureC: Double, apparentTemperatureC: Double, condition: String, symbol: String,
                humidity: Int, windKph: Double, windDirectionDeg: Double, pressureMb: Double, uvIndex: Int,
                visibilityKm: Double, isDaylight: Bool, attribution: WeatherAttribution) {
        self.asOf = asOf
        self.temperatureC = temperatureC
        self.apparentTemperatureC = apparentTemperatureC
        self.condition = condition
        self.symbol = symbol
        self.humidity = humidity
        self.windKph = windKph
        self.windDirectionDeg = windDirectionDeg
        self.pressureMb = pressureMb
        self.uvIndex = uvIndex
        self.visibilityKm = visibilityKm
        self.isDaylight = isDaylight
        self.attribution = attribution
    }

    private enum CodingKeys: String, CodingKey {
        case condition, symbol, humidity, attribution
        case asOf = "as_of"
        case temperatureC = "temperature_c"
        case apparentTemperatureC = "apparent_temperature_c"
        case windKph = "wind_kph"
        case windDirectionDeg = "wind_direction_deg"
        case pressureMb = "pressure_mb"
        case uvIndex = "uv_index"
        case visibilityKm = "visibility_km"
        case isDaylight = "is_daylight"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        asOf = try c.decodeDate(forKey: .asOf) ?? Date()
        temperatureC = try c.decode(Double.self, forKey: .temperatureC)
        apparentTemperatureC = try c.decode(Double.self, forKey: .apparentTemperatureC)
        condition = try c.decode(String.self, forKey: .condition)
        symbol = try c.decode(String.self, forKey: .symbol)
        humidity = try c.decode(Int.self, forKey: .humidity)
        windKph = try c.decode(Double.self, forKey: .windKph)
        windDirectionDeg = try c.decode(Double.self, forKey: .windDirectionDeg)
        pressureMb = try c.decode(Double.self, forKey: .pressureMb)
        uvIndex = try c.decode(Int.self, forKey: .uvIndex)
        visibilityKm = try c.decode(Double.self, forKey: .visibilityKm)
        isDaylight = try c.decode(Bool.self, forKey: .isDaylight)
        attribution = try c.decode(WeatherAttribution.self, forKey: .attribution)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeDate(asOf, forKey: .asOf)
        try c.encode(temperatureC, forKey: .temperatureC)
        try c.encode(apparentTemperatureC, forKey: .apparentTemperatureC)
        try c.encode(condition, forKey: .condition)
        try c.encode(symbol, forKey: .symbol)
        try c.encode(humidity, forKey: .humidity)
        try c.encode(windKph, forKey: .windKph)
        try c.encode(windDirectionDeg, forKey: .windDirectionDeg)
        try c.encode(pressureMb, forKey: .pressureMb)
        try c.encode(uvIndex, forKey: .uvIndex)
        try c.encode(visibilityKm, forKey: .visibilityKm)
        try c.encode(isDaylight, forKey: .isDaylight)
        try c.encode(attribution, forKey: .attribution)
    }
}

public struct DailyForecastRow: Codable, Equatable, Sendable {
    /// `yyyy-MM-dd` in the local time zone.
    public var date: String
    public var condition: String
    public var symbol: String
    public var highC: Double
    public var lowC: Double
    public var precipitationChance: Int
    public var sunrise: Date?
    public var sunset: Date?

    public init(date: String, condition: String, symbol: String, highC: Double, lowC: Double,
                precipitationChance: Int, sunrise: Date?, sunset: Date?) {
        self.date = date
        self.condition = condition
        self.symbol = symbol
        self.highC = highC
        self.lowC = lowC
        self.precipitationChance = precipitationChance
        self.sunrise = sunrise
        self.sunset = sunset
    }

    private enum CodingKeys: String, CodingKey {
        case date, condition, symbol, sunrise, sunset
        case highC = "high_c"
        case lowC = "low_c"
        case precipitationChance = "precipitation_chance"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        date = try c.decode(String.self, forKey: .date)
        condition = try c.decode(String.self, forKey: .condition)
        symbol = try c.decode(String.self, forKey: .symbol)
        highC = try c.decode(Double.self, forKey: .highC)
        lowC = try c.decode(Double.self, forKey: .lowC)
        precipitationChance = try c.decode(Int.self, forKey: .precipitationChance)
        sunrise = try c.decodeDate(forKey: .sunrise)
        sunset = try c.decodeDate(forKey: .sunset)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(date, forKey: .date)
        try c.encode(condition, forKey: .condition)
        try c.encode(symbol, forKey: .symbol)
        try c.encode(highC, forKey: .highC)
        try c.encode(lowC, forKey: .lowC)
        try c.encode(precipitationChance, forKey: .precipitationChance)
        try c.encodeDate(sunrise, forKey: .sunrise)
        try c.encodeDate(sunset, forKey: .sunset)
    }

    public static func dayString(_ date: Date, timeZone: TimeZone = .current) -> String {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = timeZone
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: date)
    }
}

public struct HourlyForecastRow: Codable, Equatable, Sendable {
    public var time: Date
    public var temperatureC: Double
    public var condition: String
    public var precipitationChance: Int

    public init(time: Date, temperatureC: Double, condition: String, precipitationChance: Int) {
        self.time = time
        self.temperatureC = temperatureC
        self.condition = condition
        self.precipitationChance = precipitationChance
    }

    private enum CodingKeys: String, CodingKey {
        case time, condition
        case temperatureC = "temperature_c"
        case precipitationChance = "precipitation_chance"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        time = try c.decodeDate(forKey: .time) ?? Date()
        temperatureC = try c.decode(Double.self, forKey: .temperatureC)
        condition = try c.decode(String.self, forKey: .condition)
        precipitationChance = try c.decode(Int.self, forKey: .precipitationChance)
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encodeDate(time, forKey: .time)
        try c.encode(temperatureC, forKey: .temperatureC)
        try c.encode(condition, forKey: .condition)
        try c.encode(precipitationChance, forKey: .precipitationChance)
    }
}

public struct ForecastRow: Codable, Equatable, Sendable {
    public var days: [DailyForecastRow]
    public var hourlyNext24: [HourlyForecastRow]
    public var attribution: WeatherAttribution

    public init(days: [DailyForecastRow], hourlyNext24: [HourlyForecastRow], attribution: WeatherAttribution) {
        self.days = days
        self.hourlyNext24 = hourlyNext24
        self.attribution = attribution
    }

    private enum CodingKeys: String, CodingKey {
        case days, attribution
        case hourlyNext24 = "hourly_next_24"
    }
}

// MARK: - Service protocol

public protocol WeatherService: Sendable {
    func current(latitude: Double, longitude: Double) async throws -> CurrentWeatherRow
    /// `days` is already clamped to 1...10 by the command layer.
    func forecast(latitude: Double, longitude: Double, days: Int) async throws -> ForecastRow
}

/// Parsed `weather.*` arguments.
public struct WeatherQuery: Equatable, Sendable {
    public static let defaultDays = 7
    public static let maxDays = 10

    public var latitude: Double
    public var longitude: Double
    public var days: Int

    public init(latitude: Double, longitude: Double, days: Int = WeatherQuery.defaultDays) {
        self.latitude = latitude
        self.longitude = longitude
        self.days = days
    }

    /// `lat`/`lon` required and in range; `days` defaults to 7 and is capped
    /// at 10 (fewer than 1 is `invalid_args`).
    public static func parse(_ args: Args) throws -> WeatherQuery {
        let lat = try args.requiredDouble("lat")
        let lon = try args.requiredDouble("lon")
        guard (-90...90).contains(lat) else { throw BridgeError.invalidArgs("'lat' must be between -90 and 90") }
        guard (-180...180).contains(lon) else { throw BridgeError.invalidArgs("'lon' must be between -180 and 180") }
        let days = try args.int("days") ?? defaultDays
        guard days >= 1 else { throw BridgeError.invalidArgs("'days' must be at least 1") }
        return WeatherQuery(latitude: lat, longitude: lon, days: min(days, maxDays))
    }
}

// MARK: - Command registration

public func registerWeatherCommands(_ router: CommandRouter, service: some WeatherService) async {
    await router.register("weather.current") { raw in
        let q = try WeatherQuery.parse(Args(raw))
        return try JSONValue(encoding: try await service.current(latitude: q.latitude, longitude: q.longitude))
    }
    await router.register("weather.forecast") { raw in
        let q = try WeatherQuery.parse(Args(raw))
        return try JSONValue(encoding: try await service.forecast(latitude: q.latitude, longitude: q.longitude, days: q.days))
    }
}

/// Rounds to `places` decimals so wire values stay short.
func rounded(_ value: Double, places: Int = 1) -> Double {
    let factor = pow(10.0, Double(places))
    return (value * factor).rounded() / factor
}

/// A 0…1 fraction as a whole percentage.
func percent(_ fraction: Double) -> Int {
    Int((min(max(fraction, 0), 1) * 100).rounded())
}

// MARK: - The real service

#if canImport(WeatherKit)
import CoreLocation
import WeatherKit

/// `WeatherService` on WeatherKit. Needs the `com.apple.developer.weatherkit`
/// entitlement (App ID capability); anything WeatherKit throws is
/// `weather_unavailable` with the underlying message.
public final class WKWeatherService: WeatherService {
    public init() {}

    public func current(latitude: Double, longitude: Double) async throws -> CurrentWeatherRow {
        let location = CLLocation(latitude: latitude, longitude: longitude)
        let (current, attribution) = try await mapping {
            async let weather = WeatherKit.WeatherService.shared.weather(for: location, including: .current)
            async let attribution = WeatherKit.WeatherService.shared.attribution
            return try await (weather, attribution)
        }
        return Self.row(current, attribution: Self.row(attribution))
    }

    public func forecast(latitude: Double, longitude: Double, days: Int) async throws -> ForecastRow {
        let location = CLLocation(latitude: latitude, longitude: longitude)
        let (daily, hourly, attribution) = try await mapping {
            async let weather = WeatherKit.WeatherService.shared.weather(for: location, including: .daily, .hourly)
            async let attribution = WeatherKit.WeatherService.shared.attribution
            let (daily, hourly) = try await weather
            return try await (daily, hourly, attribution)
        }
        let now = Date()
        return ForecastRow(
            days: daily.forecast.prefix(days).map(Self.row),
            hourlyNext24: hourly.forecast
                .filter { $0.date >= now.addingTimeInterval(-3600) && $0.date < now.addingTimeInterval(24 * 3600) }
                .prefix(24).map(Self.row),
            attribution: Self.row(attribution))
    }

    private func mapping<T>(_ body: () async throws -> T) async throws -> T {
        do {
            return try await body()
        } catch let error as BridgeError {
            throw error
        } catch {
            throw BridgeError.weatherUnavailable("WeatherKit: \(error.localizedDescription) (\(String(describing: error)))")
        }
    }

    static func row(_ a: WeatherKit.WeatherAttribution) -> WeatherAttribution {
        WeatherAttribution(
            legalURL: a.legalPageURL.absoluteString, serviceName: a.serviceName,
            logoLightURL: a.combinedMarkLightURL.absoluteString, logoDarkURL: a.combinedMarkDarkURL.absoluteString)
    }

    static func row(_ w: CurrentWeather, attribution: WeatherAttribution) -> CurrentWeatherRow {
        CurrentWeatherRow(
            asOf: w.date,
            temperatureC: rounded(w.temperature.converted(to: .celsius).value),
            apparentTemperatureC: rounded(w.apparentTemperature.converted(to: .celsius).value),
            condition: w.condition.rawValue, symbol: w.symbolName, humidity: percent(w.humidity),
            windKph: rounded(w.wind.speed.converted(to: .kilometersPerHour).value),
            windDirectionDeg: rounded(w.wind.direction.converted(to: .degrees).value, places: 0),
            pressureMb: rounded(w.pressure.converted(to: .millibars).value),
            uvIndex: w.uvIndex.value,
            visibilityKm: rounded(w.visibility.converted(to: .kilometers).value),
            isDaylight: w.isDaylight, attribution: attribution)
    }

    static func row(_ d: DayWeather) -> DailyForecastRow {
        DailyForecastRow(
            date: DailyForecastRow.dayString(d.date), condition: d.condition.rawValue, symbol: d.symbolName,
            highC: rounded(d.highTemperature.converted(to: .celsius).value),
            lowC: rounded(d.lowTemperature.converted(to: .celsius).value),
            precipitationChance: percent(d.precipitationChance), sunrise: d.sun.sunrise, sunset: d.sun.sunset)
    }

    static func row(_ h: HourWeather) -> HourlyForecastRow {
        HourlyForecastRow(
            time: h.date, temperatureC: rounded(h.temperature.converted(to: .celsius).value),
            condition: h.condition.rawValue, precipitationChance: percent(h.precipitationChance))
    }
}
#endif
