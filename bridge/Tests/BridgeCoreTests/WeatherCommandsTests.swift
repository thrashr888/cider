import Foundation
import XCTest
@testable import BridgeCore

/// Canned `WeatherService`; records the queries it was asked.
actor FakeWeatherService: WeatherService {
    static let attribution = WeatherAttribution(
        legalURL: "https://weatherkit.apple.com/legal-attribution.html", serviceName: "Apple Weather",
        logoLightURL: "https://example.com/light.png", logoDarkURL: nil)
    static let asOf = DateCoding.parse("2026-09-02T12:00:00-07:00")!

    private(set) var queries: [(lat: Double, lon: Double, days: Int?)] = []
    var failure: BridgeError?

    func setFailure(_ error: BridgeError?) { failure = error }

    func current(latitude: Double, longitude: Double) async throws -> CurrentWeatherRow {
        queries.append((latitude, longitude, nil))
        if let failure { throw failure }
        return CurrentWeatherRow(
            asOf: Self.asOf, temperatureC: 18.4, apparentTemperatureC: 17.9, condition: "partlyCloudy",
            symbol: "cloud.sun", humidity: 71, windKph: 14.8, windDirectionDeg: 270, pressureMb: 1015.2, uvIndex: 5,
            visibilityKm: 16.1, isDaylight: true, attribution: Self.attribution)
    }

    func forecast(latitude: Double, longitude: Double, days: Int) async throws -> ForecastRow {
        queries.append((latitude, longitude, days))
        if let failure { throw failure }
        let dayRows = (0..<days).map { i in
            let day = Self.asOf.addingTimeInterval(Double(i) * 86_400)
            return DailyForecastRow(
                date: DailyForecastRow.dayString(day), condition: "clear", symbol: "sun.max", highC: 22 + Double(i),
                lowC: 12, precipitationChance: i * 10, sunrise: i == 0 ? nil : day.addingTimeInterval(-5 * 3600),
                sunset: day.addingTimeInterval(7 * 3600))
        }
        let hours = (0..<24).map { i in
            HourlyForecastRow(time: Self.asOf.addingTimeInterval(Double(i) * 3600), temperatureC: 15 + Double(i) / 2,
                              condition: "clear", precipitationChance: 0)
        }
        return ForecastRow(days: dayRows, hourlyNext24: hours, attribution: Self.attribution)
    }
}

final class WeatherCommandsTests: XCTestCase {
    private var router: CommandRouter!
    private var service: FakeWeatherService!

    override func setUp() async throws {
        service = FakeWeatherService()
        router = CommandRouter(version: "test")
        await registerWeatherCommands(router, service: service)
    }

    private func call(_ cmd: String, _ args: [String: JSONValue] = [:]) async -> Response {
        await router.dispatch(Request(id: 1, cmd: cmd, args: args))
    }

    func testCurrentRowShape() async throws {
        let response = await call("weather.current", ["lat": 37.75, "lon": -122.49])
        XCTAssertTrue(response.ok, response.error?.message ?? "")
        let data = response.data!
        XCTAssertEqual(Set(data.objectValue!.keys), [
            "as_of", "temperature_c", "apparent_temperature_c", "condition", "symbol", "humidity", "wind_kph",
            "wind_direction_deg", "pressure_mb", "uv_index", "visibility_km", "is_daylight", "attribution",
        ])
        XCTAssertEqual(data["as_of"], .string(DateCoding.format(FakeWeatherService.asOf)))
        XCTAssertEqual(data["temperature_c"], 18.4)
        XCTAssertEqual(data["humidity"], 71)
        XCTAssertEqual(data["wind_direction_deg"], 270)
        XCTAssertEqual(data["is_daylight"], true)
        XCTAssertEqual(data["attribution"], [
            "legal_url": "https://weatherkit.apple.com/legal-attribution.html", "service_name": "Apple Weather",
            "logo_light_url": "https://example.com/light.png", "logo_dark_url": nil,
        ])
        let query = await service.queries.last
        XCTAssertEqual(query?.lat, 37.75)
        XCTAssertEqual(query?.lon, -122.49)
        XCTAssertNil(query?.days)

        let decoded = try JSONDecoder().decode(CurrentWeatherRow.self, from: try JSONEncoder().encode(data))
        let expected = try await service.current(latitude: 0, longitude: 0)
        XCTAssertEqual(decoded, expected)
    }

    func testForecastShapeAndDaysClamp() async throws {
        let response = await call("weather.forecast", ["lat": 37.75, "lon": -122.49])
        XCTAssertTrue(response.ok, response.error?.message ?? "")
        let data = response.data!
        XCTAssertEqual(Set(data.objectValue!.keys), ["days", "hourly_next_24", "attribution"])
        XCTAssertEqual(data["days"]?.arrayValue?.count, 7)
        XCTAssertEqual(data["hourly_next_24"]?.arrayValue?.count, 24)
        let day = data["days"]![0]!
        XCTAssertEqual(Set(day.objectValue!.keys), ["date", "condition", "symbol", "high_c", "low_c", "precipitation_chance", "sunrise", "sunset"])
        XCTAssertEqual(day["date"], .string(DailyForecastRow.dayString(FakeWeatherService.asOf)))
        XCTAssertEqual(day["sunrise"], .null)
        XCTAssertEqual(day["high_c"], 22)
        let hour = data["hourly_next_24"]![1]!
        XCTAssertEqual(Set(hour.objectValue!.keys), ["time", "temperature_c", "condition", "precipitation_chance"])
        XCTAssertEqual(hour["temperature_c"], 15.5)
        XCTAssertEqual(data["attribution"]?["service_name"], "Apple Weather")

        // days: default 7, capped at 10, at least 1.
        var query = await service.queries.last
        XCTAssertEqual(query?.days, 7)
        _ = await call("weather.forecast", ["lat": 1, "lon": 2, "days": 3])
        query = await service.queries.last
        XCTAssertEqual(query?.days, 3)
        let capped = await call("weather.forecast", ["lat": 1, "lon": 2, "days": 30])
        XCTAssertEqual(capped.data?["days"]?.arrayValue?.count, 10)
        query = await service.queries.last
        XCTAssertEqual(query?.days, 10)
        let zero = await call("weather.forecast", ["lat": 1, "lon": 2, "days": 0])
        XCTAssertEqual(zero.error?.code, "invalid_args")
    }

    func testArgumentValidation() async {
        for args: [String: JSONValue] in [[:], ["lat": 1], ["lon": 1], ["lat": "north", "lon": 0], ["lat": 91, "lon": 0], ["lat": 0, "lon": -181]] {
            let response = await call("weather.current", args)
            XCTAssertEqual(response.error?.code, "invalid_args", "\(args)")
        }
        // Numeric strings are accepted.
        let strings = await call("weather.current", ["lat": "37.75", "lon": "-122.49"])
        XCTAssertTrue(strings.ok)
    }

    func testWeatherErrorsKeepTheirCode() async {
        await service.setFailure(.weatherUnavailable("WeatherKit: not entitled"))
        let response = await call("weather.current", ["lat": 1, "lon": 2])
        XCTAssertEqual(response, .failure(id: 1, error: .weatherUnavailable("WeatherKit: not entitled")))
    }

    func testHelpers() {
        XCTAssertEqual(percent(0.714), 71)
        XCTAssertEqual(percent(1.5), 100)
        XCTAssertEqual(percent(-1), 0)
        XCTAssertEqual(rounded(18.449), 18.4)
        XCTAssertEqual(rounded(269.6, places: 0), 270)
        XCTAssertEqual(DailyForecastRow.dayString(DateCoding.parse("2026-09-02T23:30:00-07:00")!,
                                                  timeZone: TimeZone(identifier: "America/Los_Angeles")!), "2026-09-02")
    }
}
