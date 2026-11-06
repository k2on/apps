// Where a thing let go comes to rest (ArkUICore's `Momentum`), as Apple's
// Designing Fluid Interfaces projects it, and the speed a spring takes over
// from the finger: what the mini player's swipe, the full player's pull
// down and the queue's reorder land by.
@testable import Harken
import ArkUICore
import Foundation
import XCTest

final class MomentumTests: XCTestCase {
    /// A scroll view's decay: 1000 points a second goes on about 499 more,
    /// the snappy rate about 99. Falsified by the textbook's `v²/2a`.
    func testALetGoThingTravelsOnAsAScrollViewDecelerates() {
        XCTAssertTrue(abs(Momentum.project(1000) - 499) < 0.001)
        XCTAssertTrue(abs(Momentum.project(1000, rate: Momentum.snappy) - 99) < 0.001)
        XCTAssertTrue(abs(Momentum.project(-1000) + 499) < 0.001)
        XCTAssertEqual(Momentum.project(0), 0)
    }

    /// The target is the one nearest where it would rest, not where it was
    /// let go: from 100 toward 0 or 300, a flick down lands on 300, held
    /// still it lands on 0.
    func testTheTargetIsChosenFromWhereItWouldRest() {
        XCTAssertEqual(Momentum.target([0, 300], at: 100, velocity: 500), 300)
        XCTAssertEqual(Momentum.target([0, 300], at: 100, velocity: 0), 0)
        XCTAssertEqual(Momentum.target([0, 300], at: 200, velocity: -500), 0)
        XCTAssertNil(Momentum.target([], at: 0, velocity: 0))
    }

    /// The finger's speed as a share of the way left, and nothing when no
    /// way is left (rather than a division by nought).
    func testTheSpeedHandedOffIsAShareOfTheWayLeft() {
        XCTAssertEqual(Momentum.relative(500, from: 100, to: 300), 2.5)
        XCTAssertEqual(Momentum.relative(-500, from: 100, to: 0), 5)
        XCTAssertEqual(Momentum.relative(500, from: 1, to: 1), 0)
    }

    /// Past an edge, a drag is drawn less and less far: about half the way
    /// at first, never as far as the room it has, and the drag's own way.
    func testADragPastItsEdgeIsRubberBanded() {
        XCTAssertTrue(abs(Momentum.band(10, limit: 800) - 5.46) < 0.01)
        XCTAssertTrue(Momentum.band(10_000, limit: 800) < 800)
        XCTAssertEqual(Momentum.band(-100, limit: 800), -Momentum.band(100, limit: 800))
        XCTAssertEqual(Momentum.band(0, limit: 800), 0)
        XCTAssertEqual(Momentum.band(50, limit: 0), 0)
    }
}
