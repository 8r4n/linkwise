import CryptoKit
import Foundation
import IMAP
import MIME
import SMTP
import Testing

@Test func linkwiseAttachmentRoundTrip() async throws {
    let bytes = Data((0..<1400).map { UInt8(truncatingIfNeeded: $0 * 31 + 7) })
    let text = Part(data: Data("Simulator attachment fixture".utf8), contentType: .text(.plain, .ascii))
    let attachment = Part(
        data: bytes.base64EncodedData(options: [.lineLength76Characters, .endLineWithCarriageReturn, .endLineWithLineFeed]),
        contentDisposition: .attachment(.init(filename: "simulator.bin")),
        contentTransferEncoding: .base64,
        contentType: .application("octet-stream")
    )
    let body = try Body(part: Part(parts: [text, attachment]))
    let sender = SMTPClient(.init(.startTLS, hostname: "localhost", username: "bench", password: "fixture", port: 12525))
    try await sender.send(.init(
        sender: "KN6MQS@winlink.org", recipients: ["W1AW@winlink.org"],
        subject: "Thunderbird simulator attachment", body: body
    ))
    print("LINKWISE SMTP submission succeeded")
    let reader = IMAPClient(.init(.tls, hostname: "localhost", port: 11143))
    try await reader.connect()
    try await reader.login(username: "bench", password: "fixture")
    _ = try await reader.examine(mailbox: "INBOX")
    for _ in 0..<90 {
        let messages = try await reader.fetch(attributes: [.bodySection(peek: true, .complete, nil)])
        if let body = messages.messages.first?.body {
            let parts = try body.part.parts
            let part = try #require(parts.first { part in
                if case .attachment(let file) = part.contentDisposition { return file.filename == "simulator.bin" }
                return false
            })
            let received = try #require(Data(base64Encoded: part.data, options: .ignoreUnknownCharacters))
            #expect(received == bytes)
            let hash = SHA256.hash(data: received).map { String(format: "%02x", $0) }.joined()
            print("LINKWISE IMAP SHA-256 \(hash)")
            try await reader.logout()
            return
        }
        try await Task.sleep(for: .seconds(1))
    }
    Issue.record("IMAP did not expose the forwarded message")
}
