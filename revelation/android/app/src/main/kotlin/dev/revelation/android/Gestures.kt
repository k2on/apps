// The gesture state machine: one place that says what the surface is doing,
// so the view's touch handling is a set of questions to it rather than a
// set of flags.
//
//     idle ──scroll──▶ dragging ──fling──▶ flinging ──settled──▶ idle
//       │                 │ up                 │
//       │                 ▼                    │ down: stopped where it is
//       │               idle ◀─────────────────┘
//       ├──scale begin──▶ pinching ──scale end──▶ idle (or animating, to snap)
//       └──animate──▶ animating ──settled──▶ idle
//
// A touch during any motion (a fling, a fly-to, a snap) ends it and takes
// the camera from where it is: `down` says whether there was one to stop.
// Once a touch has pinched, the one finger left after the other lifts does
// not drag, so the document does not jump to follow it.
package dev.revelation.android

class Gestures {
    enum class Phase { IDLE, DRAGGING, FLINGING, PINCHING, ANIMATING }

    var phase: Phase = Phase.IDLE
        private set

    private var pinchedThisTouch = false

    /** Whether frames are needed without a finger driving them. */
    val moving: Boolean get() = phase == Phase.FLINGING || phase == Phase.ANIMATING

    /** A first finger down. True if a motion was running and must be stopped where it is. */
    fun down(): Boolean {
        val stop = moving
        phase = Phase.IDLE
        pinchedThisTouch = false
        return stop
    }

    /** A scroll past the touch slop. True if it should move the camera. */
    fun scroll(): Boolean {
        if (phase == Phase.PINCHING || pinchedThisTouch) return false
        if (phase == Phase.IDLE) phase = Phase.DRAGGING
        return phase == Phase.DRAGGING
    }

    /** A fling at the end of a drag. True if the fling should run. */
    fun fling(): Boolean {
        if (phase != Phase.DRAGGING) return false
        phase = Phase.FLINGING
        return true
    }

    /** The last finger up without a fling. */
    fun up() {
        if (phase == Phase.DRAGGING || phase == Phase.PINCHING) phase = Phase.IDLE
    }

    fun scaleBegin() {
        phase = Phase.PINCHING
        pinchedThisTouch = true
    }

    /** The pinch over; [snap] if the camera then animates to a snapped scale. */
    fun scaleEnd(snap: Boolean) {
        if (phase != Phase.PINCHING) return
        phase = if (snap) Phase.ANIMATING else Phase.IDLE
    }

    /** A fly-to or a zoom step begins. Refused while a finger is driving the camera. */
    fun animate(): Boolean {
        if (phase == Phase.DRAGGING || phase == Phase.PINCHING) return false
        phase = Phase.ANIMATING
        return true
    }

    /** The fling or the animation reached its end. */
    fun settled() {
        if (moving) phase = Phase.IDLE
    }
}
