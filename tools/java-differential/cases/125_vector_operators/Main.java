import java.util.ArrayList;
import java.util.HashMap;

public class Main {
    record V3(long x, long y, long z) implements Comparable<V3> {
        V3 plus(V3 o) { return new V3(x + o.x, y + o.y, z + o.z); }
        V3 minus(V3 o) { return new V3(x - o.x, y - o.y, z - o.z); }
        V3 times(long k) { return new V3(x * k, y * k, z * k); }
        long dot(V3 o) { return x * o.x + y * o.y + z * o.z; }
        public int compareTo(V3 o) { return Long.compare(this.dot(this), o.dot(o)); }
        V3 cross(V3 o) {
            return new V3(y * o.z - z * o.y, z * o.x - x * o.z, x * o.y - y * o.x);
        }
        public String toString() { return "(" + x + ", " + y + ", " + z + ")"; }
    }

    static class M3 {
        long[][] m;

        M3(long[][] m) { this.m = m; }

        static M3 rotZ() {
            long[][] r = {{0, -1, 0}, {1, 0, 0}, {0, 0, 1}};
            return new M3(r);
        }

        V3 times(V3 v) {
            return new V3(
                m[0][0] * v.x() + m[0][1] * v.y() + m[0][2] * v.z(),
                m[1][0] * v.x() + m[1][1] * v.y() + m[1][2] * v.z(),
                m[2][0] * v.x() + m[2][1] * v.y() + m[2][2] * v.z());
        }

        M3 times(M3 o) {
            long[][] out = new long[3][3];
            for (int i = 0; i < 3; i++) {
                for (int j = 0; j < 3; j++) {
                    long s = 0;
                    for (int k = 0; k < 3; k++) {
                        s += m[i][k] * o.m[k][j];
                    }
                    out[i][j] = s;
                }
            }
            return new M3(out);
        }
    }

    public static void main(String[] args) {
        V3 a = new V3(1, 2, 3);
        V3 b = new V3(4, -5, 6);
        System.out.println(a.plus(b));
        System.out.println(b.minus(a));
        System.out.println(a.times(3));
        System.out.println(a.dot(b));
        System.out.println(a.cross(b));
        System.out.println(a.plus(b).times(2).minus(a));

        M3 rot = M3.rotZ();
        V3 p = new V3(1, 0, 0);
        System.out.println(rot.times(p));
        System.out.println(rot.times(rot).times(p));
        System.out.println(rot.times(rot).times(rot).times(rot).times(p));

        ArrayList<V3> vs = new ArrayList<>();
        vs.add(new V3(3, 0, 0));
        vs.add(new V3(1, 1, 1));
        vs.add(new V3(0, 0, 5));
        vs.add(new V3(2, 2, 0));
        vs.sort(null);
        String line = "";
        for (V3 v : vs) {
            line += v + " ";
        }
        System.out.println(line.trim());
        System.out.println(new V3(1, 1, 1).compareTo(new V3(2, 0, 0)) < 0);

        HashMap<V3, String> seen = new HashMap<>();
        seen.put(new V3(1, 2, 3), "a");
        System.out.println(seen.containsKey(a) + " " + a.equals(new V3(1, 2, 3)));
    }
}
