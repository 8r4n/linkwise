import AVFoundation
import Foundation
import os
import ServiceManagement

let logger = Logger(subsystem: "io.github.8r4n.linkwise.modem", category: "station")
let agent = SMAppService.agent(plistName: "io.github.8r4n.linkwise.modem.station.plist")

@MainActor
func main() async throws {
    switch CommandLine.arguments.dropFirst().first {
    case "enable":
        guard await AVCaptureDevice.requestAccess(for: .audio) else {
            throw NSError(domain: "Linkwise microphone permission", code: 1)
        }
        try agent.register()
        print("Linkwise station registration: \(agent.status)")
    case "disable":
        try await agent.unregister()
    case "status":
        print(agent.status)
    case "receive":
        let config = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Linkwise/station.json", isDirectory: false)
        guard AVCaptureDevice.authorizationStatus(for: .audio) == .authorized else {
            logger.error("Grant microphone permission using the Linkwise Station enable command.")
            Foundation.exit(1)
        }
        let binary = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/linkwise-modemd").path(percentEncoded: false)
        let paths: [String] = [binary, "--serve-station", config.path(percentEncoded: false)]
        var arguments: [UnsafeMutablePointer<CChar>?] = paths.map { path in
            path.withCString { strdup($0) }
        }
        defer { for argument in arguments { free(argument) } }
        guard arguments.allSatisfy({ $0 != nil }) else { throw POSIXError(.ENOMEM) }
        arguments.append(nil)
        setenv("LINKWISE_UNIFIED_LOG", "1", 1)
        _ = execv(binary, arguments)
        throw POSIXError(POSIXErrorCode(rawValue: errno) ?? .EIO)
    default:
        print("Usage: LinkwiseStation enable|disable|status")
    }
}

do {
    try await main()
} catch {
    logger.error("\(error.localizedDescription, privacy: .public)")
    fputs("\(error.localizedDescription)\n", stderr)
    Foundation.exit(1)
}
