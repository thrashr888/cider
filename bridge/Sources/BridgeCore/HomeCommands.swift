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

    await router.register("home.run_scene") { raw in
        let args = Args(raw)
        try await service.runScene(home: try args.string("home"), scene: try args.requiredString("scene"))
        return ["ran": true]
    }

    await router.register("home.set") { raw in
        let args = Args(raw)
        let result = try await service.set(
            home: try args.string("home"), accessory: try args.requiredString("accessory"),
            service: try args.string("service"), characteristic: try args.requiredString("characteristic"),
            value: try args.requiredValue("value"))
        return try JSONValue(encoding: result)
    }

    await router.register("home.trigger_create_timer") { raw in
        let args = Args(raw)
        let row = try await service.createTimerTrigger(
            home: try args.string("home"), name: try args.requiredString("name"),
            fireAt: try args.requiredDate("fire_at"), recurrence: try Recurrence.parse(args.value("recurrence")),
            scenes: try args.requiredStringArray("scenes"))
        return try JSONValue(encoding: row)
    }

    await router.register("home.trigger_set_enabled") { raw in
        let args = Args(raw)
        let row = try await service.setTriggerEnabled(
            home: try args.string("home"), trigger: try args.requiredString("trigger"),
            enabled: try args.requiredBool("enabled"))
        return try JSONValue(encoding: row)
    }

    await router.register("home.trigger_delete") { raw in
        let args = Args(raw)
        try await service.deleteTrigger(home: try args.string("home"), trigger: try args.requiredString("trigger"))
        return ["deleted": true]
    }
}
