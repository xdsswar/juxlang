public class Main {
    enum Planet {
        MERCURY,
        VENUS,
        EARTH
    }

    static String describe(Planet p) {
        return switch (p) {
            case MERCURY -> "small and hot";
            case VENUS -> "hot and thick";
            case EARTH -> "home";
        };
    }

    public static void main(String[] args) {
        System.out.println(describe(Planet.MERCURY));
        System.out.println(describe(Planet.VENUS));
        System.out.println(describe(Planet.EARTH));
        Planet a = Planet.EARTH;
        Planet b = Planet.EARTH;
        Planet c = Planet.VENUS;
        System.out.println(a == b);
        System.out.println(a == c);
        System.out.println(describe(a));
    }
}
