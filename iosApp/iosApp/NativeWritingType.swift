import UIKit

enum NativeWritingType {
    static var publicBody: UIFont { font(size: 18, style: .body) }
    static var publicHeadline: UIFont { font(size: 24, style: .title2) }

    static func showsCharacterCount(_ length: Int, limit: Int32) -> Bool {
        let maximum = Int(limit)
        return maximum > 0 && length >= maximum - min(100, maximum / 5)
    }

    private static func font(size: CGFloat, style: UIFont.TextStyle) -> UIFont {
        guard let serif = UIFont(name: "GeistSerif-Regular", size: size) else {
            return .preferredFont(forTextStyle: style)
        }
        return UIFontMetrics(forTextStyle: style).scaledFont(for: serif)
    }
}
