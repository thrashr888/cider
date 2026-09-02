#if canImport(UIKit) && canImport(HomeKit)
import UIKit

/// UIKit on this SDK requires scene-lifecycle adoption and traps at launch
/// without it. The bridge has no window, so the delegate only exists to
/// satisfy that requirement.
final class SceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(
        _ scene: UIScene,
        willConnectTo session: UISceneSession,
        options connectionOptions: UIScene.ConnectionOptions
    ) {
        // Faceless: no window is created.
    }
}
#endif
