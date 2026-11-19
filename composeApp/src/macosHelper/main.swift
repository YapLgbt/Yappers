import Foundation

let arguments = CommandLine.arguments
guard arguments.count == 3 else { exit(2) }
let services = AppleNearbyServices()
services.request(operation: arguments[1], query: arguments[2]) { result, error in
    if let result { print(result); exit(0) }
    FileHandle.standardError.write(Data((error ?? "Service unavailable").utf8))
    exit(1)
}
RunLoop.main.run()
