// swift-tools-version: 5.9
// The swift-tools-version declares the minimum version of Swift required to build this package.

import PackageDescription

let package = Package(
    name: "desktop_screenstate",
    platforms: [
        .macOS("10.14")
    ],
    products: [
        .library(name: "desktop-screenstate", targets: ["desktop_screenstate"])
    ],
    dependencies: [],
    targets: [
        .target(
            name: "desktop_screenstate",
            dependencies: [],
            resources: [
                .process("PrivacyInfo.xcprivacy")
            ]
        )
    ]
)
