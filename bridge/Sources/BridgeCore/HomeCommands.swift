import Foundation

/// Registers `ping` and every `home.*` command against `service`. Shared by
/// the Catalyst app (with `HMHomeKitService`) and the tests (with the fake).
public func registerHomeCommands(_ router: CommandRouter, service: some HomeKitService) async {
    let version = router.version

    await router.register("ping") { _ in
        let status = await service.status()
        return [
            "version": .string(version),
            "homekit_authorized": .bool(status.authorized),
            "homes": .number(Double(status.homes)),
        ]
    }

    await router.register("home.homes") { _ in
        try JSONValue(encoding: try await service.homes())
    }

    await router.register("home.rooms") { raw in
        let args = Args(raw)
        return try JSONValue(encoding: try await service.rooms(home: try args.string("home")))
    }

    await router.register("home.accessories") { raw in
        let args = Args(raw)
        return try JSONValue(encoding: try await service.accessories(
            home: try args.string("home"), room: try args.string("room")))
    }

    await router.register("home.scenes") { raw in
        let args = Args(raw)
        return try JSONValue(encoding: try await service.scenes(home: try args.string("home")))
    }

    await router.register("home.triggers") { raw in
        let args = Args(raw)
        return try JSONValue(encoding: try await service.triggers(home: try args.string("home")))
    }
}
