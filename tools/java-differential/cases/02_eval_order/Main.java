public class Main {
    static int n = 0;

    static int bump(int by) {
        n = n + by;
        return n;
    }

    public static void main(String[] args) {
        System.out.println(bump(1) + bump(10) * bump(100));
        System.out.println(n);
        int k = 5;
        System.out.println(k++ + --k);
        System.out.println(k);
        int a = 0;
        a += 3;
        a *= 4;
        a -= 2;
        a /= 2;
        System.out.println(a);
        boolean hit = false;
        if (false && bump(1000) > 0) { hit = true; }
        System.out.println(n);
        if (true || bump(1000) > 0) { hit = true; }
        System.out.println(n);
        System.out.println(hit);
    }
}
