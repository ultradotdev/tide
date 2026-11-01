// Folding review fixture. Leave the caret here, scroll to a function below,
// then click its gutter arrow repeatedly. The viewport should stay in place.
// Also try word wrap, hidden line numbers, nested folds and a caret inside a fold.

const padding_0_00 = 0;
const padding_0_01 = 1;
const padding_0_02 = 2;
const padding_0_03 = 3;
const padding_0_04 = 4;
const padding_0_05 = 5;
const padding_0_06 = 6;
const padding_0_07 = 7;
const padding_0_08 = 8;
const padding_0_09 = 9;
const padding_0_10 = 10;
const padding_0_11 = 11;
const padding_0_12 = 12;
const padding_0_13 = 13;
const padding_0_14 = 14;
const padding_0_15 = 15;
const padding_0_16 = 16;
const padding_0_17 = 17;
const padding_0_18 = 18;
const padding_0_19 = 19;
const padding_0_20 = 20;
const padding_0_21 = 21;
const padding_0_22 = 22;
const padding_0_23 = 23;
const padding_0_24 = 24;
const padding_0_25 = 25;
const padding_0_26 = 26;
const padding_0_27 = 27;
const padding_0_28 = 28;
const padding_0_29 = 29;
const padding_0_30 = 30;
const padding_0_31 = 31;
const padding_0_32 = 32;
const padding_0_33 = 33;
const padding_0_34 = 34;
const padding_0_35 = 35;
const padding_0_36 = 36;
const padding_0_37 = 37;
const padding_0_38 = 38;
const padding_0_39 = 39;
const padding_0_40 = 40;
const padding_0_41 = 41;
const padding_0_42 = 42;
const padding_0_43 = 43;
const padding_0_44 = 44;
const padding_0_45 = 45;
const padding_0_46 = 46;
const padding_0_47 = 47;
const padding_0_48 = 48;
const padding_0_49 = 49;
const padding_0_50 = 50;
const padding_0_51 = 51;
const padding_0_52 = 52;
const padding_0_53 = 53;
const padding_0_54 = 54;
const padding_0_55 = 55;
const padding_0_56 = 56;
const padding_0_57 = 57;
const padding_0_58 = 58;
const padding_0_59 = 59;
const padding_0_60 = 60;
const padding_0_61 = 61;
const padding_0_62 = 62;
const padding_0_63 = 63;
const padding_0_64 = 64;

export function firstBelowViewport(items: number[]) {
    const literal = "braces in strings: { [ ( ) ] }";
    // Delimiters in this comment are not scopes: { [ (
    /* A multiline comment with a fake close:
       } ] )
    */
    for (const item of items) {
        if (item > 0) {
            const nested = [item, { value: item + 1 }];
            console.log(nested);
        }
    }
    return "firstBelowViewport: a deliberately long line to exercise wrapped screen rows while folding a function that was outside the initial viewport.";
}

const padding_1_00 = 0;
const padding_1_01 = 1;
const padding_1_02 = 2;
const padding_1_03 = 3;
const padding_1_04 = 4;
const padding_1_05 = 5;
const padding_1_06 = 6;
const padding_1_07 = 7;
const padding_1_08 = 8;
const padding_1_09 = 9;
const padding_1_10 = 10;
const padding_1_11 = 11;
const padding_1_12 = 12;
const padding_1_13 = 13;
const padding_1_14 = 14;
const padding_1_15 = 15;
const padding_1_16 = 16;
const padding_1_17 = 17;
const padding_1_18 = 18;
const padding_1_19 = 19;
const padding_1_20 = 20;
const padding_1_21 = 21;
const padding_1_22 = 22;
const padding_1_23 = 23;
const padding_1_24 = 24;
const padding_1_25 = 25;
const padding_1_26 = 26;
const padding_1_27 = 27;
const padding_1_28 = 28;
const padding_1_29 = 29;
const padding_1_30 = 30;
const padding_1_31 = 31;
const padding_1_32 = 32;
const padding_1_33 = 33;
const padding_1_34 = 34;
const padding_1_35 = 35;
const padding_1_36 = 36;
const padding_1_37 = 37;
const padding_1_38 = 38;
const padding_1_39 = 39;
const padding_1_40 = 40;
const padding_1_41 = 41;
const padding_1_42 = 42;
const padding_1_43 = 43;
const padding_1_44 = 44;
const padding_1_45 = 45;
const padding_1_46 = 46;
const padding_1_47 = 47;
const padding_1_48 = 48;
const padding_1_49 = 49;
const padding_1_50 = 50;
const padding_1_51 = 51;
const padding_1_52 = 52;
const padding_1_53 = 53;
const padding_1_54 = 54;
const padding_1_55 = 55;
const padding_1_56 = 56;
const padding_1_57 = 57;
const padding_1_58 = 58;
const padding_1_59 = 59;
const padding_1_60 = 60;
const padding_1_61 = 61;
const padding_1_62 = 62;
const padding_1_63 = 63;
const padding_1_64 = 64;

export function nestedBelowViewport(items: number[]) {
    const literal = "braces in strings: { [ ( ) ] }";
    // Delimiters in this comment are not scopes: { [ (
    /* A multiline comment with a fake close:
       } ] )
    */
    for (const item of items) {
        if (item > 0) {
            const nested = [item, { value: item + 1 }];
            console.log(nested);
        }
    }
    return "nestedBelowViewport: a deliberately long line to exercise wrapped screen rows while folding a function that was outside the initial viewport.";
}

const padding_2_00 = 0;
const padding_2_01 = 1;
const padding_2_02 = 2;
const padding_2_03 = 3;
const padding_2_04 = 4;
const padding_2_05 = 5;
const padding_2_06 = 6;
const padding_2_07 = 7;
const padding_2_08 = 8;
const padding_2_09 = 9;
const padding_2_10 = 10;
const padding_2_11 = 11;
const padding_2_12 = 12;
const padding_2_13 = 13;
const padding_2_14 = 14;
const padding_2_15 = 15;
const padding_2_16 = 16;
const padding_2_17 = 17;
const padding_2_18 = 18;
const padding_2_19 = 19;
const padding_2_20 = 20;
const padding_2_21 = 21;
const padding_2_22 = 22;
const padding_2_23 = 23;
const padding_2_24 = 24;
const padding_2_25 = 25;
const padding_2_26 = 26;
const padding_2_27 = 27;
const padding_2_28 = 28;
const padding_2_29 = 29;
const padding_2_30 = 30;
const padding_2_31 = 31;
const padding_2_32 = 32;
const padding_2_33 = 33;
const padding_2_34 = 34;
const padding_2_35 = 35;
const padding_2_36 = 36;
const padding_2_37 = 37;
const padding_2_38 = 38;
const padding_2_39 = 39;
const padding_2_40 = 40;
const padding_2_41 = 41;
const padding_2_42 = 42;
const padding_2_43 = 43;
const padding_2_44 = 44;
const padding_2_45 = 45;
const padding_2_46 = 46;
const padding_2_47 = 47;
const padding_2_48 = 48;
const padding_2_49 = 49;
const padding_2_50 = 50;
const padding_2_51 = 51;
const padding_2_52 = 52;
const padding_2_53 = 53;
const padding_2_54 = 54;
const padding_2_55 = 55;
const padding_2_56 = 56;
const padding_2_57 = 57;
const padding_2_58 = 58;
const padding_2_59 = 59;
const padding_2_60 = 60;
const padding_2_61 = 61;
const padding_2_62 = 62;
const padding_2_63 = 63;
const padding_2_64 = 64;

export function lastBelowViewport(items: number[]) {
    const literal = "braces in strings: { [ ( ) ] }";
    // Delimiters in this comment are not scopes: { [ (
    /* A multiline comment with a fake close:
       } ] )
    */
    for (const item of items) {
        if (item > 0) {
            const nested = [item, { value: item + 1 }];
            console.log(nested);
        }
    }
    return "lastBelowViewport: a deliberately long line to exercise wrapped screen rows while folding a function that was outside the initial viewport.";
}

