public class Main {
    static int bump(String tag) {
        System.out.println("eval " + tag);
        return 1;
    }

    static boolean yes(String tag) {
        System.out.println("test " + tag);
        return true;
    }

    static boolean no(String tag) {
        System.out.println("test " + tag);
        return false;
    }

    public static void main(String[] args) {
        int i = 0;
        System.out.println(i++);
        System.out.println(i);
        System.out.println(++i);
        System.out.println(i--);
        System.out.println(--i);
        int a = bump("a") + bump("b") * bump("c");
        System.out.println(a);
        if (no("x") && yes("y")) {
            System.out.println("both");
        } else {
            System.out.println("short");
        }
        if (yes("p") || no("q")) {
            System.out.println("or-true");
        }
        int t = no("t") ? bump("then") : bump("else");
        System.out.println(t);
        int[] arr = new int[3];
        int k = 0;
        arr[k++] = k;
        System.out.println(arr[0]);
        System.out.println(k);
        int[] two = new int[2];
        two[0] = 5;
        two[1] = 9;
        two[0] = two[1];
        System.out.println(two[0]);
        System.out.println(two[1]);
    }
}
