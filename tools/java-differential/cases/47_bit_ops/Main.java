public class Main {
    static int mask(int v, int m) {
        return v & m;
    }

    public static void main(String[] args) {
        int a = 11;
        int b = 6;
        System.out.println(a & b);
        System.out.println(a | b);
        System.out.println(a ^ b);
        System.out.println(~a);
        System.out.println(a << 3);
        System.out.println(a >> 1);
        int neg = -32;
        System.out.println(neg >> 2);
        System.out.println(neg << 1);
        System.out.println(mask(255, 15));
        System.out.println(mask(-1, 7));
        int flags = 0;
        flags = flags | 4;
        flags = flags | 1;
        System.out.println(flags);
        flags = flags & ~4;
        System.out.println(flags);
        long big = 1;
        System.out.println(big << 40);
        System.out.println((big << 40) >> 8);
        System.out.println(7 ^ 7);
        System.out.println(0 | 9);
    }
}
