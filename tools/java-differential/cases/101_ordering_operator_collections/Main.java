import java.util.ArrayList;
import java.util.Collections;
import java.util.Map;
import java.util.PriorityQueue;
import java.util.TreeMap;

class Version implements Comparable<Version> {
    int major;
    int minor;

    Version(int major, int minor) {
        this.major = major;
        this.minor = minor;
    }

    public int compareTo(Version other) {
        if (major != other.major) {
            return major - other.major;
        }
        return minor - other.minor;
    }

    public String toString() { return major + "." + minor; }
}

class Score implements Comparable<Score> {
    String name;
    double points;

    Score(String name, double points) {
        this.name = name;
        this.points = points;
    }

    public int compareTo(Score other) { return Double.compare(points, other.points); }
}

public class Main {
    public static void main(String[] args) {
        ArrayList<Version> releases = new ArrayList<>();
        releases.add(new Version(2, 0));
        releases.add(new Version(1, 5));
        releases.add(new Version(1, 12));
        Collections.sort(releases);
        for (Version v : releases) {
            System.out.println(v);
        }

        System.out.println(new Version(1, 0).compareTo(new Version(1, 1)) < 0);
        System.out.println(new Version(3, 0).compareTo(new Version(2, 9)) >= 0);

        TreeMap<Version, String> notes = new TreeMap<>();
        notes.put(new Version(3, 1), "rewrite");
        notes.put(new Version(1, 0), "first");
        notes.put(new Version(2, 4), "fixes");
        for (Map.Entry<Version, String> entry : notes.entrySet()) {
            System.out.println(entry.getKey() + ": " + entry.getValue());
        }

        PriorityQueue<Score> board = new PriorityQueue<>(Collections.reverseOrder());
        board.add(new Score("ann", 1.5));
        board.add(new Score("bob", 9.0));
        board.add(new Score("cy", 4.0));
        while (board.size() > 0) {
            Score top = board.poll();
            System.out.println(top.name + " " + top.points);
        }
    }
}
