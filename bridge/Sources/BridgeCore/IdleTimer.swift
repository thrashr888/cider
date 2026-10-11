import Foundation
import os

/// Calls `onIdle` once `timeout` passes without a `touch()`. Each touch bumps a
/// generation counter; a scheduled firing only runs if its generation is still
/// current, so there is no work item to cancel and no lock held across the call.
public final class IdleTimer: Sendable {
    public let timeout: TimeInterval
    private let schedule: @Sendable (TimeInterval, @escaping @Sendable () -> Void) -> Void
    private let onIdle: @Sendable () -> Void
    private let generation = OSAllocatedUnfairLock(initialState: 0)

    public convenience init(timeout: TimeInterval, queue: DispatchQueue = .global(), onIdle: @escaping @Sendable () -> Void) {
        self.init(timeout: timeout, schedule: { delay, work in
            queue.asyncAfter(deadline: .now() + delay, execute: work)
        }, onIdle: onIdle)
    }

    init(timeout: TimeInterval,
         schedule: @escaping @Sendable (TimeInterval, @escaping @Sendable () -> Void) -> Void,
         onIdle: @escaping @Sendable () -> Void) {
        self.timeout = timeout
        self.schedule = schedule
        self.onIdle = onIdle
    }

    /// Starts (or restarts) the countdown.
    public func start() { touch() }

    /// Records activity, pushing the deadline out by `timeout`.
    public func touch() {
        let mine = generation.withLock { gen -> Int in
            gen += 1
            return gen
        }
        schedule(timeout) { [weak self] in
            guard let self, self.generation.withLock({ $0 == mine }) else { return }
            self.onIdle()
        }
    }

    /// Cancels any pending firing until the next `touch()`.
    public func stop() {
        generation.withLock { $0 += 1 }
    }
}
