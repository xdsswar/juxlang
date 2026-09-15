public class Main {
    public static void main(String[] args) {
        long x = 88172645463325252L;
        for (int i = 0; i < 4000; i++) {
            x = x * 6364136223846793005L + 1442695040888963407L;
            int k = (int) ((x >> 3) & 63L);
            int small = (int) (x >> 40);
            double m = (double) (x >> 11);
            double v = m;
            for (int j = 0; j < k; j++) {
                if ((x & 1L) == 0L) {
                    v = v * 10.0;
                } else {
                    v = v / 10.0;
                }
            }
            if ((x & 30L) == 6L) {
                v = v / 1e300;
            }
            if ((x & 62L) == 14L) {
                v = v / 1e300 / 1e300;
            }
            float f = (float) v;

            System.out.println(x);
            System.out.println(x / 7L);
            System.out.println(x % 1000L);
            System.out.println(x >> k);
            System.out.println(x << k);
            System.out.println((int) x);
            System.out.println((short) x);
            System.out.println((byte) x);
            System.out.println(small / 3);
            System.out.println(small % 5);
            System.out.println(small + x);
            System.out.println(v);
            System.out.println(f);
            System.out.println(-v);
            System.out.println(v * 0.1);
            System.out.println(f * 3.0f);
            System.out.println(k + v);
            System.out.println(small * 1.5);
            System.out.println((long) v);
            System.out.println((int) v);
            System.out.println((long) f);
            System.out.println((double) f);
            System.out.println(Math.floor(v));
            System.out.println(Math.ceil(v));
            System.out.println(Math.sqrt(Math.abs(v)));
            System.out.println(v < m);
            System.out.println(v == m);
            System.out.println("v=" + v);
            System.out.println("f=" + f + " v=" + v);
        }
    }
}
