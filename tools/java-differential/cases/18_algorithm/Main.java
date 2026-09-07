public class Main {
    static void bubble(int[] a) {
        for (int i = 0; i < a.length; i++) {
            for (int j = 0; j < a.length - 1 - i; j++) {
                if (a[j] > a[j + 1]) {
                    int t = a[j];
                    a[j] = a[j + 1];
                    a[j + 1] = t;
                }
            }
        }
    }

    static int bsearch(int[] a, int target) {
        int lo = 0;
        int hi = a.length - 1;
        while (lo <= hi) {
            int mid = (lo + hi) / 2;
            if (a[mid] == target) { return mid; }
            if (a[mid] < target) { lo = mid + 1; } else { hi = mid - 1; }
        }
        return -1;
    }

    public static void main(String[] args) {
        int[] a = new int[8];
        a[0] = 5; a[1] = 3; a[2] = 9; a[3] = 1;
        a[4] = 7; a[5] = 2; a[6] = 8; a[7] = 6;
        bubble(a);
        String out = "";
        for (int v : a) {
            out = out + v + ",";
        }
        System.out.println(out);
        System.out.println(bsearch(a, 7));
        System.out.println(bsearch(a, 1));
        System.out.println(bsearch(a, 100));

        int primes = 0;
        for (int n = 2; n < 50; n++) {
            boolean p = true;
            for (int d = 2; d * d <= n; d++) {
                if (n % d == 0) { p = false; }
            }
            if (p) { primes = primes + 1; }
        }
        System.out.println(primes);
    }
}
