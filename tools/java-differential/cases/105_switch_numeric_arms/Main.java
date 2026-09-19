public class Main {
    static long total(int kind, int count, long big) {
        return switch (kind) {
            case 1 -> count;
            case 2 -> big;
            default -> count * 2;
        };
    }

    static double ratio(int kind, int n) {
        return switch (kind) {
            case 1 -> n;
            case 2 -> n / 4.0;
            default -> 7;
        };
    }

    public static void main(String[] args) {
        long big = 5000000000L;
        System.out.println(total(1, 3, big));
        System.out.println(total(2, 3, big));
        System.out.println(total(9, 3, big));
        System.out.println(ratio(1, 3));
        System.out.println(ratio(2, 3));
        System.out.println(ratio(9, 3));
        int k = 2;
        var mixed = switch (k) { case 2 -> big + 1; default -> k; };
        System.out.println(mixed);
        var chosen = k > 1 ? 1.5 : k;
        System.out.println(chosen);
    }
}
