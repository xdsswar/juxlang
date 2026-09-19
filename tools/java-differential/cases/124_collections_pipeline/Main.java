import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

public class Main {
    record Hit(String path, int status, long ms) {}

    static List<Hit> parse(String[] lines) {
        List<Hit> out = new ArrayList<>();
        for (String line : lines) {
            String[] parts = line.split(" ");
            if (parts.length != 3) {
                continue;
            }
            try {
                out.add(new Hit(parts[0], Integer.parseInt(parts[1]), Long.parseLong(parts[2])));
            } catch (NumberFormatException e) {
                // not a log line
            }
        }
        return out;
    }

    static List<Hit> errorsOnly(List<Hit> source) {
        List<Hit> out = new ArrayList<>();
        for (Hit h : source) {
            if (h.status() >= 500) {
                out.add(h);
            }
        }
        return out;
    }

    public static void main(String[] args) {
        String[] log = {
            "/home 200 12", "/api/users 200 40", "/api/users 500 900", "garbage line",
            "/home 304 3", "/api/orders 200 55", "/api/orders 503 1200", "/api/users 200 38",
            "/static/app.js 200 4", "/api/orders 200 61", "/home 200 15", "/api/users 502 700"
        };

        TreeMap<String, Long> byPath = new TreeMap<>();
        TreeMap<String, Integer> counts = new TreeMap<>();
        HashSet<Integer> statuses = new HashSet<>();
        ArrayDeque<Long> window = new ArrayDeque<>();
        long windowSum = 0;
        ArrayList<Long> slowest = new ArrayList<>();

        for (Hit h : parse(log)) {
            byPath.put(h.path(), byPath.getOrDefault(h.path(), 0L) + h.ms());
            counts.put(h.path(), counts.getOrDefault(h.path(), 0) + 1);
            statuses.add(h.status());
            window.addLast(h.ms());
            windowSum += h.ms();
            if (window.size() > 3) {
                windowSum -= window.pollFirst();
            }
            slowest.add(h.ms());
        }

        for (Map.Entry<String, Long> e : byPath.entrySet()) {
            int n = counts.getOrDefault(e.getKey(), 1);
            System.out.println(e.getKey() + " total " + e.getValue() + "ms avg " + (e.getValue() / n) + "ms over " + n);
        }
        System.out.println("distinct statuses " + statuses.size() + ", has 404 " + statuses.contains(404) + ", has 503 " + statuses.contains(503));
        System.out.println("last 3 window sum " + windowSum);

        Collections.sort(slowest);
        String top = "";
        for (int k = slowest.size(); k > slowest.size() - 3; k--) {
            top += slowest.get(k - 1) + " ";
        }
        System.out.println("slowest " + top.trim());

        String errors = "";
        for (Hit h : errorsOnly(parse(log))) {
            errors += h.path() + ":" + h.status() + " ";
        }
        System.out.println("errors " + errors.trim());
    }
}
