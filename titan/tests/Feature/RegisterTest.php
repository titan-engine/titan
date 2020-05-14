<?php
namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class RegisterTest extends TestCase {
    use RefreshDatabase;

    public function testRegisterPageLoads() {
        $this->get('/register')
            ->assertStatus(200)
            ->assertSeeText("Register");

    }
}
