#if canImport(UIKit) && canImport(HomeKit)
import BridgeCore
import UIKit

/// Cider Bridge: a faceless (`LSUIElement`) Mac Catalyst app that serves the
/// bridge protocol on the RFC socket with the real HomeKit and WeatherKit
/// services, and quits
/// after ten idle minutes. No menu bar item in v1: Catalyst cannot link AppKit
/// directly and a plugin bundle is not worth it for one status icon.
@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    static let idleTimeout: TimeInterval = 10 * 60

    private var server: LineSocketServer?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        let router = CommandRouter(version: BridgeInfo.version) { AppDelegate.scheduleQuit() }
        let idle = IdleTimer(timeout: Self.idleTimeout) { AppDelegate.scheduleQuit() }
        let server = LineSocketServer(router: router, idleTimer: idle)
        self.server = server

        let service = HMHomeKitService()
        Task { @MainActor in
            await registerHomeCommands(router, service: service)
            #if canImport(WeatherKit)
            await registerWeatherCommands(router, service: WKWeatherService())
            #endif
            do {
                try server.start()
                NSLog("cider-bridge: listening on %@", server.path)
            } catch {
                NSLog("cider-bridge: cannot start socket server: %@", String(describing: error))
                exit(1)
            }
        }
        return true
    }

    func application(
        _ application: UIApplication,
        configurationForConnecting connectingSceneSession: UISceneSession,
        options: UIScene.ConnectionOptions
    ) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Default", sessionRole: connectingSceneSession.role)
        configuration.delegateClass = SceneDelegate.self
        return configuration
    }

    func applicationWillTerminate(_ application: UIApplication) {
        server?.stop()
    }

    /// Quit shortly after the current reply has been written, from any context.
    nonisolated static func scheduleQuit() {
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(250))
            (UIApplication.shared.delegate as? AppDelegate)?.server?.stop()
            exit(0)
        }
    }
}
#endif
