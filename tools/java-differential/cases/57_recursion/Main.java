public class Main {
    static int fib(int n) {
        if (n < 2) {
            return n;
        }
        return fib(n - 1) + fib(n - 2);
    }

    static int gcd(int a, int b) {
        if (b == 0) {
            return a;
        }
        return gcd(b, a % b);
    }

    static int sumTo(int n) {
        if (n <= 0) {
            return 0;
        }
        return n + sumTo(n - 1);
    }

    static String reverse(String s) {
        if (s.length() <= 1) {
            return s;
        }
        return reverse(s.substring(1)) + s.substring(0, 1);
    }

    static boolean isEven(int n) {
        if (n == 0) {
            return true;
        }
        return isOdd(n - 1);
    }

    static boolean isOdd(int n) {
        if (n == 0) {
            return false;
        }
        return isEven(n - 1);
    }

    public static void main(String[] args) {
        System.out.println(fib(10));
        System.out.println(fib(20));
        System.out.println(gcd(48, 18));
        System.out.println(gcd(17, 5));
        System.out.println(sumTo(100));
        System.out.println(reverse("stressed"));
        System.out.println(isEven(10));
        System.out.println(isOdd(7));
    }
}
