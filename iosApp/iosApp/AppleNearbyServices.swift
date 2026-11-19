import Foundation
import CoreLocation
import MapKit
#if os(iOS)
import Yappers
#endif

/// Apple Maps resolves towns; only rounded coordinates cross the UI/network boundary.
final class AppleNearbyServices: NSObject, CLLocationManagerDelegate {
    private var manager: CLLocationManager?
    private var pendingFix: CheckedContinuation<CLLocation, Error>?

    func request(operation: String, query: String, onResult: @escaping (String?, String?) -> Void) {
        Task { @MainActor in
            do {
                let data: Data
                switch operation {
                case "enroll":
                    data = try await enrollment()
                case "search":
                    let request = MKLocalSearch.Request()
                    request.naturalLanguageQuery = query
                    request.resultTypes = .address
                    if #available(macOS 15, iOS 18, *) {
                        request.addressFilter = MKAddressFilter(including: [.locality, .subLocality])
                    }
                    let response: MKLocalSearch.Response
                    do { response = try await MKLocalSearch(request: request).start() }
                    catch let error as MKError where error.code == .placemarkNotFound {
                        onResult("[]", nil); return
                    }
                    let areas = response.mapItems.compactMap { item -> [String: Any]? in
                        let place = item.placemark
                        guard let city = place.locality ?? place.subLocality ?? item.name else { return nil }
                        return self.area(place.coordinate, [city, place.administrativeArea, place.isoCountryCode])
                    }
                    data = try JSONSerialization.data(withJSONObject: areas)
                case "locate":
                    let location = try await currentLocation()
                    let coarse = CLLocation(latitude: (location.coordinate.latitude * 100).rounded() / 100,
                                            longitude: (location.coordinate.longitude * 100).rounded() / 100)
                    let place = try? await CLGeocoder().reverseGeocodeLocation(coarse).first
                    let center = area(coarse.coordinate, [place?.locality ?? "Current area", place?.administrativeArea, place?.isoCountryCode])
                    data = try JSONSerialization.data(withJSONObject: center)
                default: throw failure("Unknown location request.")
                }
                onResult(String(decoding: data, as: UTF8.self), nil)
            } catch {
                onResult(nil, operation == "locate" ? "Location unavailable. Allow location access in Settings or search for a town." :
                    operation == "search" ? "Couldn’t search Apple Maps. Check your connection and try again." : "Couldn’t reach yap.lgbt.")
            }
        }
    }

    private func area(_ coordinate: CLLocationCoordinate2D, _ names: [String?]) -> [String: Any] {
        var labels = [String]()
        for case let name? in names where !name.isEmpty && !labels.contains(name) { labels.append(name) }
        return ["label": labels.joined(separator: ", "), "lat100": Int((coordinate.latitude * 100).rounded()),
                "lon100": Int((coordinate.longitude * 100).rounded())]
    }

    private func failure(_ message: String) -> NSError {
        NSError(domain: "Yap.Nearby", code: 1, userInfo: [NSLocalizedDescriptionKey: message])
    }

    private func enrollment() async throws -> Data {
        #if DEBUG && os(iOS)
        if ProcessInfo.processInfo.arguments.contains("--netkit-ui-test"),
           let fixture = ProcessInfo.processInfo.environment["YAP_TEST_ENROLLMENT"] {
            return Data(fixture.utf8)
        }
        #endif
        // Both names belong to the same Fly app. The second works during custom DNS setup.
        for host in ["yap.lgbt", "yap-lgbt-home.fly.dev"] {
            let url = URL(string: "https://\(host)/.well-known/yap-home.json")!
            var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 8)
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            do {
                let (bytes, response) = try await URLSession.shared.bytes(for: request)
                guard let http = response as? HTTPURLResponse, http.statusCode == 200,
                      http.url?.host == host, http.url?.scheme == "https" else { continue }
                var data = Data()
                for try await byte in bytes {
                    guard data.count < 16_384 else { throw failure("Enrollment document too large.") }
                    data.append(byte)
                }
                return data
            } catch { continue }
        }
        throw failure("Enrollment unavailable.")
    }

    @MainActor
    private func currentLocation() async throws -> CLLocation {
        guard pendingFix == nil else { throw failure("Already locating.") }
        let locator = CLLocationManager()
        manager = locator
        locator.desiredAccuracy = kCLLocationAccuracyKilometer
        locator.delegate = self
        return try await withCheckedThrowingContinuation { continuation in
            pendingFix = continuation
            authorize(locator)
            DispatchQueue.main.asyncAfter(deadline: .now() + 30) { [weak self, weak locator] in
                guard let self, let locator, self.manager === locator else { return }
                self.finish(.failure(self.failure("Location timed out.")))
            }
        }
    }
    private func authorize(_ locator: CLLocationManager) {
        switch locator.authorizationStatus {
        case .notDetermined:
            #if os(macOS)
            locator.requestLocation()
            #else
            locator.requestWhenInUseAuthorization()
            #endif
        case .authorizedAlways, .authorizedWhenInUse: locator.requestLocation()
        default: finish(.failure(failure("Location access unavailable.")))
        }
    }
    func locationManagerDidChangeAuthorization(_ manager: CLLocationManager) {
        if self.manager === manager && manager.authorizationStatus != .notDetermined { authorize(manager) }
    }
    func locationManager(_ manager: CLLocationManager, didUpdateLocations locations: [CLLocation]) {
        guard self.manager === manager, let location = locations.last, location.horizontalAccuracy >= 0 else { return }
        finish(.success(location))
    }
    func locationManager(_ manager: CLLocationManager, didFailWithError error: Error) {
        if self.manager === manager { finish(.failure(error)) }
    }
    private func finish(_ result: Result<CLLocation, Error>) {
        manager?.delegate = nil
        manager = nil
        let pending = pendingFix
        pendingFix = nil
        pending?.resume(with: result)
    }
}
#if os(iOS)
extension AppleNearbyServices: NearbyServices {}
#endif
