import SwiftUI

/// Procedural cover art: a gradient, a pattern, and a symbol or set type.
struct Artwork: View {
    let art: Art
    var circle = false

    var body: some View {
        GeometryReader { geo in
            let s = min(geo.size.width, geo.size.height)
            let shape = circle ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: max(3, s * 0.045), style: .continuous))
            ZStack {
                if art.style == .type {
                    art.paper
                    TypeCover(art: art, size: s)
                } else {
                    LinearGradient(colors: [art.top, art.bottom], startPoint: .topLeading, endPoint: .bottomTrailing)
                    Pattern(art: art)
                    Image(systemName: art.symbol)
                        .font(.system(size: s * 0.34, weight: .light))
                        .foregroundStyle(.white.opacity(0.88))
                        .shadow(color: .black.opacity(0.15), radius: s * 0.02, y: s * 0.01)
                }
            }
            .frame(width: geo.size.width, height: geo.size.height)
            .clipShape(shape)
            .overlay(shape.stroke(Color.primary.opacity(0.08), lineWidth: 0.5))
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

private struct TypeCover: View {
    let art: Art
    let size: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: size * 0.03) {
            Rectangle().fill(art.ink).frame(width: size * 0.18, height: max(1, size * 0.012))
            Text(art.label ?? "")
                .font(.system(size: size * 0.11, weight: .semibold, design: .serif))
                .foregroundStyle(art.ink)
                .lineLimit(3)
                .minimumScaleFactor(0.5)
            Spacer(minLength: 0)
            HStack {
                Text(art.sublabel?.uppercased() ?? "")
                    .font(.system(size: size * 0.055, weight: .medium))
                    .kerning(size * 0.008)
                    .foregroundStyle(art.ink.opacity(0.75))
                Spacer(minLength: 0)
                Image(systemName: art.symbol)
                    .font(.system(size: size * 0.09, weight: .light))
                    .foregroundStyle(Color(hue: art.hue2, saturation: 0.6, brightness: 0.7))
            }
        }
        .padding(size * 0.09)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

private struct Pattern: View {
    let art: Art

    var body: some View {
        Canvas { context, size in
            let s = min(size.width, size.height)
            switch art.style {
            case .rings:
                for i in 0..<7 {
                    let r = s * (0.12 + 0.13 * Double(i))
                    let rect = CGRect(x: size.width * 0.72 - r, y: size.height * 0.78 - r, width: r * 2, height: r * 2)
                    context.stroke(Path(ellipseIn: rect), with: .color(.white.opacity(0.16)), lineWidth: s * 0.022)
                }
            case .blobs:
                context.addFilter(.blur(radius: s * 0.09))
                context.fill(Path(ellipseIn: CGRect(x: -s * 0.15, y: s * 0.45, width: s * 0.85, height: s * 0.85)),
                             with: .color(Color(hue: art.hue2, saturation: 0.6, brightness: 0.95).opacity(0.75)))
                context.fill(Path(ellipseIn: CGRect(x: s * 0.45, y: -s * 0.2, width: s * 0.75, height: s * 0.75)),
                             with: .color(Color(hue: art.hue, saturation: 0.35, brightness: 1).opacity(0.6)))
            case .stripes:
                var x = -s
                while x < s * 2 {
                    var p = Path()
                    p.move(to: CGPoint(x: x, y: 0))
                    p.addLine(to: CGPoint(x: x + s * 0.6, y: s))
                    context.stroke(p, with: .color(.white.opacity(0.12)), lineWidth: s * 0.05)
                    x += s * 0.16
                }
            case .type:
                break
            }
        }
    }
}

/// A member of the household: their initial on their colour.
struct Avatar: View {
    let member: Member
    var size: CGFloat = 36

    var body: some View {
        Text(member.initials)
            .font(.system(size: size * 0.44, weight: .semibold, design: .rounded))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(
                LinearGradient(colors: [Color(hue: member.hue, saturation: 0.45, brightness: 0.85),
                                        Color(hue: member.hue, saturation: 0.75, brightness: 0.6)],
                               startPoint: .top, endPoint: .bottom),
                in: Circle())
            .accessibilityHidden(true)
    }
}

/// A shelf's title, with an optional See All link.
struct ShelfHeader: View {
    let title: String
    var route: Route? = nil

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title).font(.title2.bold())
            Spacer()
            if let route {
                NavigationLink("See All", value: route).font(.body)
            }
        }
        .accessibilityAddTraits(.isHeader)
    }
}

/// The animated bars shown on the playing row.
struct NowPlayingIndicator: View {
    var playing: Bool

    var body: some View {
        Image(systemName: "waveform")
            .symbolEffect(.variableColor.iterative, isActive: playing && !Launch.frozen)
            .foregroundStyle(.tint)
            .accessibilityLabel(playing ? "Playing" : "Paused")
    }
}
